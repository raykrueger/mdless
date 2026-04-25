// Copyright 2025 Ray Krueger <raykrueger@gmail.com>
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use pulldown_cmark::{Alignment, Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use std::fs;
use std::path::Path;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style as SyntectStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use crate::error::Result;

pub struct MarkdownRenderer {
    content: String,
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

/// Stateful builder that consumes pulldown-cmark events and produces ratatui `Text`.
/// Holds a reference to `MarkdownRenderer` to access syntax-highlighting helpers.
struct MarkdownLineBuilder<'a> {
    renderer: &'a MarkdownRenderer,
    lines: Vec<Line<'static>>,
    current_line: Vec<Span<'static>>,
    in_code_block: bool,
    code_block_language: String,
    code_block_content: String,
    in_heading: bool,
    heading_level: u8,
    in_emphasis: bool,
    in_strong: bool,
    last_was_empty_line: bool,
    // Table state
    in_table_head: bool,
    in_table_cell: bool,
    table_alignments: Vec<Alignment>,
    table_header: Vec<String>,
    table_rows: Vec<Vec<String>>,
    table_current_row: Vec<String>,
    table_current_cell: String,
}

impl<'a> MarkdownLineBuilder<'a> {
    fn new(renderer: &'a MarkdownRenderer) -> Self {
        Self {
            renderer,
            lines: Vec::new(),
            current_line: Vec::new(),
            in_code_block: false,
            code_block_language: String::new(),
            code_block_content: String::new(),
            in_heading: false,
            heading_level: 0,
            in_emphasis: false,
            in_strong: false,
            last_was_empty_line: true,
            in_table_head: false,
            in_table_cell: false,
            table_alignments: Vec::new(),
            table_header: Vec::new(),
            table_rows: Vec::new(),
            table_current_row: Vec::new(),
            table_current_cell: String::new(),
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.handle_start_tag(tag),
            Event::End(tag_end) => self.handle_end_tag(tag_end),
            Event::Text(text) => self.handle_text(&text),
            Event::Code(code) => self.handle_code(&code),
            Event::SoftBreak | Event::HardBreak if !self.current_line.is_empty() => {
                self.handle_break();
            }
            _ => {}
        }
    }

    fn handle_start_tag(&mut self, tag: Tag) {
        match tag {
            Tag::Heading { level, .. } => {
                self.flush_current_line();
                if !self.last_was_empty_line && !self.lines.is_empty() {
                    self.lines.push(Line::from(""));
                }
                self.in_heading = true;
                self.heading_level = level as u8;
            }
            Tag::CodeBlock(lang) => {
                self.in_code_block = true;
                self.code_block_language = match lang {
                    pulldown_cmark::CodeBlockKind::Indented => String::new(),
                    pulldown_cmark::CodeBlockKind::Fenced(lang_str) => lang_str.to_string(),
                };
                self.code_block_content.clear();
                self.flush_current_line();
                if !self.last_was_empty_line {
                    self.lines.push(Line::from(""));
                }
            }
            Tag::Emphasis => self.in_emphasis = true,
            Tag::Strong => self.in_strong = true,
            Tag::Paragraph => {}
            Tag::List(_) if !self.current_line.is_empty() => {
                self.flush_current_line();
            }
            Tag::Item => {
                self.flush_current_line();
                self.current_line.push(Span::styled(
                    "• ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            Tag::Table(alignments) => {
                self.table_alignments = alignments;
                self.table_header.clear();
                self.table_rows.clear();
                if !self.last_was_empty_line {
                    self.lines.push(Line::from(""));
                }
            }
            Tag::TableHead => {
                self.in_table_head = true;
                self.table_current_row.clear();
            }
            Tag::TableRow => self.table_current_row.clear(),
            Tag::TableCell => {
                self.in_table_cell = true;
                self.table_current_cell.clear();
            }
            _ => {}
        }
    }

    fn handle_end_tag(&mut self, tag_end: TagEnd) {
        match tag_end {
            TagEnd::Heading(_) => {
                self.in_heading = false;
                self.flush_current_line();
                self.lines.push(Line::from(""));
                self.last_was_empty_line = true;
            }
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                self.render_code_block();
                self.lines.push(Line::from(""));
                self.last_was_empty_line = true;
                self.code_block_content.clear();
                self.code_block_language.clear();
            }
            TagEnd::Emphasis => self.in_emphasis = false,
            TagEnd::Strong => self.in_strong = false,
            TagEnd::Paragraph => {
                self.flush_current_line();
                self.lines.push(Line::from(""));
                self.last_was_empty_line = true;
            }
            TagEnd::List(_) => {
                self.flush_current_line();
                self.lines.push(Line::from(""));
                self.last_was_empty_line = true;
            }
            TagEnd::Item => {
                self.flush_current_line();
                self.last_was_empty_line = false;
            }
            TagEnd::TableCell => {
                self.in_table_cell = false;
                self.table_current_row.push(self.table_current_cell.clone());
                self.table_current_cell.clear();
            }
            TagEnd::TableHead => {
                self.in_table_head = false;
                self.table_header = self.table_current_row.clone();
                self.table_current_row.clear();
            }
            TagEnd::TableRow if !self.in_table_head => {
                self.table_rows.push(self.table_current_row.clone());
                self.table_current_row.clear();
            }
            TagEnd::Table => {
                let table_lines = self.renderer.render_table(
                    &self.table_header,
                    &self.table_rows,
                    &self.table_alignments,
                );
                self.lines.extend(table_lines);
                self.lines.push(Line::from(""));
                self.last_was_empty_line = true;
            }
            _ => {}
        }
    }

    fn handle_text(&mut self, text: &str) {
        if self.in_table_cell {
            self.table_current_cell.push_str(text);
        } else if self.in_code_block {
            self.code_block_content.push_str(text);
        } else {
            let style = self.renderer.get_text_style(
                self.in_heading,
                self.heading_level,
                self.in_code_block,
                self.in_emphasis,
                self.in_strong,
            );
            self.current_line
                .push(Span::styled(text.to_string(), style));
            if !text.trim().is_empty() {
                self.last_was_empty_line = false;
            }
        }
    }

    fn handle_code(&mut self, code: &str) {
        let style = Style::default()
            .fg(Color::Yellow)
            .bg(Color::Rgb(40, 40, 40))
            .add_modifier(Modifier::BOLD);
        self.current_line
            .push(Span::styled(format!(" {} ", code), style));
        self.last_was_empty_line = false;
    }

    fn handle_break(&mut self) {
        self.flush_current_line();
        self.last_was_empty_line = false;
    }

    /// Flush the current_line buffer into lines, clearing it.
    fn flush_current_line(&mut self) {
        if !self.current_line.is_empty() {
            self.lines.push(Line::from(self.current_line.clone()));
            self.current_line.clear();
        }
    }

    /// Render the collected code block with borders and syntax highlighting.
    fn render_code_block(&mut self) {
        let highlighted_lines = self
            .renderer
            .highlight_code_block(&self.code_block_content, &self.code_block_language);

        // Top border
        self.lines.push(Line::from(vec![Span::styled(
            "┌─────────────────────────────────────────────────────────────────────────────┐",
            Style::default().fg(Color::DarkGray),
        )]));

        // Language label if present
        if !self.code_block_language.is_empty() {
            let language_display_width = self.code_block_language.chars().count();
            let padding_needed = 79_usize.saturating_sub(2 + language_display_width + 1);

            self.lines.push(Line::from(vec![
                Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    self.code_block_language.clone(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ".repeat(padding_needed), Style::default()),
                Span::styled("│", Style::default().fg(Color::DarkGray)),
            ]));
            self.lines.push(Line::from(vec![Span::styled(
                "├─────────────────────────────────────────────────────────────────────────────┤",
                Style::default().fg(Color::DarkGray),
            )]));
        }

        // Highlighted code lines
        self.lines.extend(highlighted_lines);

        // Bottom border
        self.lines.push(Line::from(vec![Span::styled(
            "└─────────────────────────────────────────────────────────────────────────────┘",
            Style::default().fg(Color::DarkGray),
        )]));
    }

    /// Finalize: flush any remaining content and return the rendered Text.
    fn finish(self) -> Text<'static> {
        let mut lines = self.lines;
        // Note: current_line is consumed by self-drop, no need to flush
        // since handle_event covers all events that leave content in current_line.
        // But to be safe, flush trailing content.
        if !self.current_line.is_empty() {
            lines.push(Line::from(self.current_line));
        }
        Text::from(lines)
    }
}

impl MarkdownRenderer {
    pub fn new() -> Self {
        Self {
            content: String::new(),
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }

    pub fn load_file<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        self.content = fs::read_to_string(path)?;
        Ok(())
    }

    pub fn render_to_text(&self) -> Text<'static> {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        let parser = Parser::new_ext(&self.content, options);

        let mut builder = MarkdownLineBuilder::new(self);
        for event in parser {
            builder.handle_event(event);
        }
        builder.finish()
    }

    fn highlight_code_block(&self, code: &str, language: &str) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        // Try to find the syntax for the given language
        let syntax = if language.is_empty() {
            self.syntax_set.find_syntax_plain_text()
        } else {
            self.syntax_set
                .find_syntax_by_token(language)
                .or_else(|| self.syntax_set.find_syntax_by_extension(language))
                .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text())
        };

        // Use a dark theme for better terminal compatibility
        let theme = &self.theme_set.themes["base16-ocean.dark"];
        let mut highlighter = HighlightLines::new(syntax, theme);

        // "│ " prefix (2) + content + padding + "│" suffix (1) = BOX_WIDTH
        const BOX_WIDTH: usize = 79;
        const INNER_WIDTH: usize = BOX_WIDTH - 3; // 76

        for line in LinesWithEndings::from(code) {
            let highlighted = highlighter
                .highlight_line(line, &self.syntax_set)
                .unwrap_or_else(|_| vec![(SyntectStyle::default(), line)]);

            let mut spans = vec![Span::styled("│ ", Style::default().fg(Color::DarkGray))];

            for (style, text) in highlighted {
                let ratatui_style = self.syntect_style_to_ratatui(style);
                let display_text = text.trim_end_matches('\n');
                if !display_text.is_empty() {
                    spans.push(Span::styled(display_text.to_string(), ratatui_style));
                }
            }

            let content_length: usize = spans
                .iter()
                .skip(1)
                .map(|s| s.content.chars().count())
                .sum();

            // Truncate lines that exceed the box inner width
            if content_length > INNER_WIDTH {
                let max_chars = INNER_WIDTH - 1; // reserve 1 for …
                let mut new_spans = vec![spans[0].clone()];
                let mut chars_left = max_chars;
                for span in spans.iter().skip(1) {
                    if chars_left == 0 {
                        break;
                    }
                    let span_len = span.content.chars().count();
                    if span_len <= chars_left {
                        new_spans.push(span.clone());
                        chars_left -= span_len;
                    } else {
                        let partial: String = span.content.chars().take(chars_left).collect();
                        new_spans.push(Span::styled(partial, span.style));
                        chars_left = 0;
                    }
                }
                new_spans.push(Span::styled("…", Style::default().fg(Color::DarkGray)));
                spans = new_spans;
            }

            let content_length: usize = spans
                .iter()
                .skip(1)
                .map(|s| s.content.chars().count())
                .sum();
            let padding_needed = BOX_WIDTH.saturating_sub(2 + content_length + 1);
            spans.push(Span::styled(" ".repeat(padding_needed), Style::default()));
            spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
            lines.push(Line::from(spans));
        }

        // If no lines were added (empty code block), add an empty line
        if lines.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("│", Style::default().fg(Color::DarkGray)),
                Span::styled(" ".repeat(76), Style::default()), // 79 - 2 - 1 = 76 spaces
                Span::styled("│", Style::default().fg(Color::DarkGray)),
            ]));
        }

        lines
    }

    fn render_table(
        &self,
        header: &[String],
        rows: &[Vec<String>],
        alignments: &[Alignment],
    ) -> Vec<Line<'static>> {
        let col_count = header.len();
        if col_count == 0 {
            return Vec::new();
        }

        // Compute column widths: max content width across header and all body rows
        let col_widths: Vec<usize> = (0..col_count)
            .map(|i| {
                let header_w = header.get(i).map(|s| s.chars().count()).unwrap_or(0);
                let body_w = rows
                    .iter()
                    .map(|row| row.get(i).map(|s| s.chars().count()).unwrap_or(0))
                    .max()
                    .unwrap_or(0);
                header_w.max(body_w).max(1)
            })
            .collect();

        let border_style = Style::default().fg(Color::DarkGray);
        let header_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);

        let top_border = Self::table_border_line(&col_widths, '┌', '┬', '┐', '─', border_style);
        let head_sep = Self::table_border_line(&col_widths, '├', '┼', '┤', '─', border_style);
        let bot_border = Self::table_border_line(&col_widths, '└', '┴', '┘', '─', border_style);

        let mut lines = vec![top_border];

        // Header row
        lines.push(Self::table_data_line(
            header,
            &col_widths,
            alignments,
            header_style,
            border_style,
        ));
        lines.push(head_sep);

        // Body rows
        for row in rows {
            lines.push(Self::table_data_line(
                row,
                &col_widths,
                alignments,
                Style::default(),
                border_style,
            ));
        }

        lines.push(bot_border);
        lines
    }

    fn table_border_line(
        col_widths: &[usize],
        left: char,
        mid: char,
        right: char,
        fill: char,
        style: Style,
    ) -> Line<'static> {
        let mut s = String::new();
        s.push(left);
        for (i, &w) in col_widths.iter().enumerate() {
            for _ in 0..w + 2 {
                s.push(fill);
            }
            if i < col_widths.len() - 1 {
                s.push(mid);
            }
        }
        s.push(right);
        Line::from(Span::styled(s, style))
    }

    fn table_data_line(
        cells: &[String],
        col_widths: &[usize],
        alignments: &[Alignment],
        cell_style: Style,
        border_style: Style,
    ) -> Line<'static> {
        let mut spans = Vec::new();
        spans.push(Span::styled("│".to_string(), border_style));
        for (i, &w) in col_widths.iter().enumerate() {
            let content = cells.get(i).map(|s| s.as_str()).unwrap_or("");
            let content_len = content.chars().count();
            let padding = w.saturating_sub(content_len);
            let alignment = alignments.get(i).unwrap_or(&Alignment::None);

            let (left_pad, right_pad) = match alignment {
                Alignment::Right => (padding, 0),
                Alignment::Center => (padding / 2, padding - padding / 2),
                _ => (0, padding),
            };

            spans.push(Span::styled(" ".to_string(), border_style));
            if left_pad > 0 {
                spans.push(Span::styled(" ".repeat(left_pad), Style::default()));
            }
            spans.push(Span::styled(content.to_string(), cell_style));
            if right_pad > 0 {
                spans.push(Span::styled(" ".repeat(right_pad), Style::default()));
            }
            spans.push(Span::styled(" ".to_string(), border_style));
            spans.push(Span::styled("│".to_string(), border_style));
        }
        Line::from(spans)
    }

    fn syntect_style_to_ratatui(&self, syntect_style: SyntectStyle) -> Style {
        let mut style = Style::default();

        // Convert foreground color
        let fg_color = syntect_style.foreground;
        style = style.fg(Color::Rgb(fg_color.r, fg_color.g, fg_color.b));

        // No background color - let the terminal's default background show through

        // Convert font style
        if syntect_style
            .font_style
            .contains(syntect::highlighting::FontStyle::BOLD)
        {
            style = style.add_modifier(Modifier::BOLD);
        }
        if syntect_style
            .font_style
            .contains(syntect::highlighting::FontStyle::ITALIC)
        {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if syntect_style
            .font_style
            .contains(syntect::highlighting::FontStyle::UNDERLINE)
        {
            style = style.add_modifier(Modifier::UNDERLINED);
        }

        style
    }

    fn get_text_style(
        &self,
        in_heading: bool,
        heading_level: u8,
        in_code_block: bool,
        in_emphasis: bool,
        in_strong: bool,
    ) -> Style {
        let mut style = Style::default();

        if in_heading {
            style = match heading_level {
                1 => style.fg(Color::Red).add_modifier(Modifier::BOLD),
                2 => style.fg(Color::Yellow).add_modifier(Modifier::BOLD),
                3 => style.fg(Color::Green).add_modifier(Modifier::BOLD),
                4 => style.fg(Color::Cyan).add_modifier(Modifier::BOLD),
                5 => style.fg(Color::Blue).add_modifier(Modifier::BOLD),
                _ => style.fg(Color::Magenta).add_modifier(Modifier::BOLD),
            };
        }

        if in_code_block {
            style = style.fg(Color::Green).bg(Color::DarkGray);
        }

        if in_emphasis {
            style = style.add_modifier(Modifier::ITALIC);
        }

        if in_strong {
            style = style.add_modifier(Modifier::BOLD);
        }

        style
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_renderer() {
        let renderer = MarkdownRenderer::new();
        assert!(renderer.content.is_empty());
    }

    #[test]
    fn test_render_simple_text() {
        let mut renderer = MarkdownRenderer::new();
        renderer.content = "Hello, world!".to_string();

        let text = renderer.render_to_text();
        assert!(!text.lines.is_empty());
    }

    #[test]
    fn test_render_heading() {
        let mut renderer = MarkdownRenderer::new();
        renderer.content = "# Main Title\n\nSome content".to_string();

        let text = renderer.render_to_text();
        assert!(text.lines.len() >= 2);
    }

    #[test]
    fn test_header_spacing_fix() {
        let mut renderer = MarkdownRenderer::new();
        renderer.content =
            "Some paragraph text.\n### Header without spacing\nMore content.".to_string();

        let text = renderer.render_to_text();

        // The rendered text should have proper spacing before the header
        // We expect: paragraph line, empty line, header line, empty line, content line
        assert!(
            text.lines.len() >= 5,
            "Should have at least 5 lines with proper spacing"
        );

        // Check that there's an empty line before the header
        let lines_text: Vec<String> = text
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect();

        // Find the header line
        let header_index = lines_text
            .iter()
            .position(|line| line.contains("Header without spacing"));
        assert!(header_index.is_some(), "Header should be found");

        let header_idx = header_index.unwrap();
        // There should be an empty line before the header (unless it's the first line)
        if header_idx > 0 {
            assert!(
                lines_text[header_idx - 1].trim().is_empty(),
                "There should be an empty line before the header"
            );
        }
    }

    #[test]
    fn test_multiple_headers_without_spacing() {
        let mut renderer = MarkdownRenderer::new();
        renderer.content =
            "Paragraph.\n### First Header\nContent.\n### Second Header\nMore content.".to_string();

        let text = renderer.render_to_text();

        // Should have proper spacing before both headers
        let lines_text: Vec<String> = text
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect();

        let first_header_idx = lines_text
            .iter()
            .position(|line| line.contains("First Header"));
        let second_header_idx = lines_text
            .iter()
            .position(|line| line.contains("Second Header"));

        assert!(
            first_header_idx.is_some() && second_header_idx.is_some(),
            "Both headers should be found"
        );

        // Both headers should have empty lines before them
        if let Some(idx) = first_header_idx
            && idx > 0
        {
            assert!(
                lines_text[idx - 1].trim().is_empty(),
                "First header should have empty line before it"
            );
        }

        if let Some(idx) = second_header_idx
            && idx > 0
        {
            assert!(
                lines_text[idx - 1].trim().is_empty(),
                "Second header should have empty line before it"
            );
        }
    }

    #[test]
    fn test_code_block_border_alignment() {
        let mut renderer = MarkdownRenderer::new();
        renderer.content = "```rust\nfn main() {}\n```".to_string();

        let text = renderer.render_to_text();

        // Find the language header line (contains "rust")
        let language_line_idx = text
            .lines
            .iter()
            .position(|line| line.spans.iter().any(|span| span.content.contains("rust")))
            .expect("Should find language header line");

        let language_line = &text.lines[language_line_idx];

        // Calculate the display width (character count, not byte count)
        let display_width: usize = language_line
            .spans
            .iter()
            .map(|s| s.content.chars().count())
            .sum();

        // The language header line should have exactly 79 display characters to match the border
        assert_eq!(
            display_width, 79,
            "Language line should be exactly 79 display characters wide"
        );

        // Check that it has the proper structure
        assert!(
            language_line.spans.len() >= 3,
            "Should have at least 3 spans"
        );
        assert_eq!(
            language_line.spans[0].content.chars().count(),
            2,
            "First span should be '│ ' (2 chars)"
        );
        assert_eq!(
            language_line.spans.last().unwrap().content.chars().count(),
            1,
            "Last span should be '│' (1 char)"
        );
    }

    #[test]
    fn test_code_block_content_line_alignment() {
        let mut renderer = MarkdownRenderer::new();
        renderer.content =
            "```rust\nfn main() {\n    println!(\"Hello, world!\");\n}\n```".to_string();

        let text = renderer.render_to_text();

        // Find all lines that contain code content (between borders)
        let code_content_lines: Vec<&Line> = text
            .lines
            .iter()
            .filter(|line| {
                // Code content lines start with "│ " and end with "│"
                line.spans.len() >= 2
                    && line.spans[0].content == "│ "
                    && line.spans.last().unwrap().content == "│"
                    && !line.spans.iter().any(|span| span.content.contains("─")) // Not a border line
                    && !line.spans.iter().any(|span| span.content.contains("rust"))
                // Not the language line
            })
            .collect();

        // Each code content line should have exactly 79 display characters
        for line in code_content_lines {
            let display_width: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();

            assert_eq!(
                display_width, 79,
                "Code content line should be exactly 79 display characters wide"
            );
        }
    }

    #[test]
    fn test_table_renders() {
        let mut renderer = MarkdownRenderer::new();
        renderer.content =
            "| Key | Action |\n| --- | --- |\n| `q` | Quit |\n| `j` | Down |".to_string();

        let text = renderer.render_to_text();
        let lines_text: Vec<String> = text
            .lines
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();

        // Should have top border, header, separator, 2 body rows, bottom border
        assert!(
            lines_text.iter().any(|l| l.starts_with('┌')),
            "missing top border"
        );
        assert!(
            lines_text.iter().any(|l| l.starts_with('├')),
            "missing header separator"
        );
        assert!(
            lines_text.iter().any(|l| l.starts_with('└')),
            "missing bottom border"
        );
        assert!(
            lines_text.iter().any(|l| l.contains("Key")),
            "missing header cell"
        );
        assert!(
            lines_text.iter().any(|l| l.contains("Quit")),
            "missing body cell"
        );
    }

    #[test]
    fn test_table_column_widths() {
        let mut renderer = MarkdownRenderer::new();
        // Second column has a longer body cell than header
        renderer.content = "| A | B |\n| --- | --- |\n| x | very long cell |".to_string();

        let text = renderer.render_to_text();
        let lines_text: Vec<String> = text
            .lines
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();

        // All border lines should be the same width
        let border_lines: Vec<&String> = lines_text
            .iter()
            .filter(|l| l.starts_with('┌') || l.starts_with('├') || l.starts_with('└'))
            .collect();

        assert!(!border_lines.is_empty());
        let width = border_lines[0].chars().count();
        for line in &border_lines {
            assert_eq!(
                line.chars().count(),
                width,
                "border lines should all be equal width"
            );
        }
    }

    #[test]
    fn test_long_code_line_truncated() {
        let mut renderer = MarkdownRenderer::new();
        // 88-char line — longer than inner box width of 76
        renderer.content =
            "```bash\nwget https://github.com/raykrueger/mdless/releases/latest/download/mdless-macos-aarch64\n```"
                .to_string();

        let text = renderer.render_to_text();

        // Every content line must be exactly 79 display chars wide
        for line in text
            .lines
            .iter()
            .filter(|l| l.spans.first().map(|s| s.content == "│ ").unwrap_or(false))
        {
            let width: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
            assert_eq!(
                width, 79,
                "code line must be exactly 79 chars wide, got {width}"
            );
        }
    }
}
