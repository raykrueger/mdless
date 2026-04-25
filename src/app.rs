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

use crossterm::{
    cursor::Show,
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use ratatui::{
    Terminal,
    backend::{Backend, CrosstermBackend},
    text::Text,
};
use std::{io, path::PathBuf, sync::mpsc, time::Duration};

use crate::error::{MdViewError, Result};
use crate::markdown::MarkdownRenderer;
use crate::ui;

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            Show
        );
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    Normal,
    Search,
}

#[derive(Debug, Clone, Default)]
pub struct SearchState {
    pub query: String,
    pub results: Vec<SearchResult>,
    pub current_result_index: Option<usize>,
    pub is_active: bool,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub line_index: usize,
    #[allow(dead_code)]
    pub char_start: usize,
    #[allow(dead_code)]
    pub char_end: usize,
}

pub struct App {
    file_path: PathBuf,
    file_name: String,
    renderer: MarkdownRenderer,
    rendered_content: Text<'static>,
    scroll_offset: usize,
    content_length: usize,
    viewport_height: usize,
    watching: bool,
    should_quit: bool,
    mode: AppMode,
    search_state: SearchState,
    status_message: Option<String>,
    #[allow(dead_code)]
    file_watcher: Option<RecommendedWatcher>,
    file_change_rx: Option<mpsc::Receiver<Option<String>>>,
}

impl App {
    pub fn new(file_path: PathBuf, watch: bool) -> Result<Self> {
        let mut renderer = MarkdownRenderer::new();
        renderer.load_file(&file_path)?;
        let rendered_content = renderer.render_to_text();
        let content_length = rendered_content.lines.len();
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();

        let (file_watcher, file_change_rx) = if watch {
            let (tx, rx) = mpsc::channel();
            let mut watcher =
                notify::recommended_watcher(move |res: notify::Result<notify::Event>| match res {
                    Ok(_) => {
                        let _ = tx.send(None);
                    }
                    Err(e) => {
                        let _ = tx.send(Some(e.to_string()));
                    }
                })?;

            watcher.watch(&file_path, RecursiveMode::NonRecursive)?;
            (Some(watcher), Some(rx))
        } else {
            (None, None)
        };

        Ok(Self {
            file_path,
            file_name,
            renderer,
            rendered_content,
            scroll_offset: 0,
            content_length,
            viewport_height: 24,
            watching: watch,
            should_quit: false,
            mode: AppMode::Normal,
            search_state: SearchState::default(),
            status_message: None,
            file_watcher,
            file_change_rx,
        })
    }

    pub fn run(&mut self) -> Result<()> {
        enable_raw_mode().map_err(|e| MdViewError::Terminal(e.to_string()))?;
        let _guard = TerminalGuard;

        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
            .map_err(|e| MdViewError::Terminal(e.to_string()))?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal =
            Terminal::new(backend).map_err(|e| MdViewError::Terminal(e.to_string()))?;

        self.run_app(&mut terminal)
    }

    fn run_app<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        loop {
            terminal
                .draw(|f| ui::draw(f, self))
                .map_err(|e| MdViewError::Terminal(e.to_string()))?;

            if self.should_quit {
                break;
            }

            // Check for file changes if watching
            if let Some(ref rx) = self.file_change_rx {
                match rx.try_recv() {
                    Ok(None) => self.reload_file()?,
                    Ok(Some(err)) => self.status_message = Some(format!("Watch error: {err}")),
                    Err(_) => {}
                }
            }

            // Handle input events
            if event::poll(Duration::from_millis(100))
                .map_err(|e| MdViewError::Terminal(e.to_string()))?
                && let Event::Key(key) =
                    event::read().map_err(|e| MdViewError::Terminal(e.to_string()))?
                && key.kind == KeyEventKind::Press
            {
                self.handle_key_event(key.code);
            }
        }

        Ok(())
    }

    fn handle_key_event(&mut self, key_code: KeyCode) {
        self.status_message = None;
        match self.mode {
            AppMode::Normal => self.handle_normal_mode_key(key_code),
            AppMode::Search => self.handle_search_mode_key(key_code),
        }
    }

    fn handle_normal_mode_key(&mut self, key_code: KeyCode) {
        match key_code {
            // Quit
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            // Start search
            KeyCode::Char('/') => {
                self.start_search();
            }
            // Next search result
            KeyCode::Char('n') => {
                self.next_search_result();
            }
            // Previous search result
            KeyCode::Char('N') => {
                self.previous_search_result();
            }
            // Reload file
            KeyCode::Char('r') => {
                if let Err(e) = self.reload_file() {
                    self.status_message = Some(format!("Reload error: {e}"));
                }
            }
            // Vim-style movement: up
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_up();
            }
            // Vim-style movement: down
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_down();
            }
            // Vim-style movement: half page up
            KeyCode::Char('u') => {
                self.scroll_half_page_up();
            }
            // Vim-style movement: half page down
            KeyCode::Char('d') => {
                self.scroll_half_page_down();
            }
            // Vim-style movement: full page up
            KeyCode::PageUp | KeyCode::Char('b') => {
                self.scroll_page_up();
            }
            // Vim-style movement: full page down
            KeyCode::PageDown | KeyCode::Char('f') => {
                self.scroll_page_down();
            }
            // Vim-style movement: top of document
            KeyCode::Home | KeyCode::Char('g') => {
                self.scroll_to_top();
            }
            // Vim-style movement: bottom of document
            KeyCode::End | KeyCode::Char('G') => {
                self.scroll_to_bottom();
            }
            // Vim-style movement: move up 5 lines
            KeyCode::Char('K') => {
                self.scroll_offset = self.scroll_offset.saturating_sub(5);
            }
            // Vim-style movement: move down 5 lines
            KeyCode::Char('J') => {
                self.scroll_offset = self
                    .scroll_offset
                    .saturating_add(5)
                    .min(self.content_length.saturating_sub(1));
            }
            // Vim-style movement: move to middle of screen
            KeyCode::Char('M') => {
                self.scroll_to_middle();
            }
            // Vim-style movement: move up 10 lines (alternative to page up)
            KeyCode::Char('U') => {
                self.scroll_offset = self.scroll_offset.saturating_sub(10);
            }
            // Vim-style movement: move down 10 lines (alternative to page down)
            KeyCode::Char('D') => {
                self.scroll_offset = self
                    .scroll_offset
                    .saturating_add(10)
                    .min(self.content_length.saturating_sub(1));
            }
            _ => {}
        }
    }

    fn handle_search_mode_key(&mut self, key_code: KeyCode) {
        match key_code {
            KeyCode::Char(c) => {
                self.search_state.query.push(c);
                self.perform_search();
            }
            KeyCode::Backspace => {
                self.search_state.query.pop();
                if self.search_state.query.is_empty() {
                    self.clear_search();
                } else {
                    self.perform_search();
                }
            }
            KeyCode::Enter => {
                self.exit_search_mode();
            }
            KeyCode::Esc => {
                self.cancel_search();
            }
            _ => {}
        }
    }

    fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    fn scroll_down(&mut self) {
        if self.scroll_offset < self.content_length.saturating_sub(1) {
            self.scroll_offset += 1;
        }
    }

    fn scroll_half_page_up(&mut self) {
        let half_page = (self.viewport_height / 2).max(1);
        self.scroll_offset = self.scroll_offset.saturating_sub(half_page);
    }

    fn scroll_half_page_down(&mut self) {
        let half_page = (self.viewport_height / 2).max(1);
        let new_offset = self.scroll_offset.saturating_add(half_page);
        self.scroll_offset = new_offset.min(self.content_length.saturating_sub(1));
    }

    fn scroll_page_up(&mut self) {
        let full_page = self.viewport_height.max(1);
        self.scroll_offset = self.scroll_offset.saturating_sub(full_page);
    }

    fn scroll_page_down(&mut self) {
        let full_page = self.viewport_height.max(1);
        let new_offset = self.scroll_offset.saturating_add(full_page);
        self.scroll_offset = new_offset.min(self.content_length.saturating_sub(1));
    }

    fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.content_length.saturating_sub(1);
    }

    fn scroll_to_middle(&mut self) {
        self.scroll_offset = self.content_length / 2;
    }

    fn reload_file(&mut self) -> Result<()> {
        self.renderer.load_file(&self.file_path)?;
        self.rendered_content = self.renderer.render_to_text();
        self.content_length = self.rendered_content.lines.len();

        // Adjust scroll offset if content is shorter
        if self.scroll_offset >= self.content_length {
            self.scroll_offset = self.content_length.saturating_sub(1);
        }

        // Clear search results since content changed
        self.clear_search();

        Ok(())
    }

    fn start_search(&mut self) {
        self.mode = AppMode::Search;
        self.search_state.query.clear();
        self.search_state.results.clear();
        self.search_state.current_result_index = None;
        self.search_state.is_active = true;
    }

    fn perform_search(&mut self) {
        if self.search_state.query.is_empty() {
            self.search_state.results.clear();
            self.search_state.current_result_index = None;
            return;
        }

        let query = self.search_state.query.to_lowercase();
        let mut results = Vec::new();

        for (line_index, line) in self.rendered_content.lines.iter().enumerate() {
            let line_text = line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .to_lowercase();

            let mut start_pos = 0;
            while let Some(pos) = line_text[start_pos..].find(&query) {
                let actual_pos = start_pos + pos;
                results.push(SearchResult {
                    line_index,
                    char_start: actual_pos,
                    char_end: actual_pos + query.len(),
                });
                start_pos = actual_pos + 1;
            }
        }

        self.search_state.results = results;
        if !self.search_state.results.is_empty() {
            self.search_state.current_result_index = Some(0);
            self.scroll_to_search_result(0);
        } else {
            self.search_state.current_result_index = None;
        }
    }

    fn next_search_result(&mut self) {
        if self.search_state.results.is_empty() {
            return;
        }

        let current_index = self.search_state.current_result_index.unwrap_or(0);
        let next_index = (current_index + 1) % self.search_state.results.len();
        self.search_state.current_result_index = Some(next_index);
        self.scroll_to_search_result(next_index);
    }

    fn previous_search_result(&mut self) {
        if self.search_state.results.is_empty() {
            return;
        }

        let current_index = self.search_state.current_result_index.unwrap_or(0);
        let prev_index = if current_index == 0 {
            self.search_state.results.len() - 1
        } else {
            current_index - 1
        };
        self.search_state.current_result_index = Some(prev_index);
        self.scroll_to_search_result(prev_index);
    }

    fn scroll_to_search_result(&mut self, result_index: usize) {
        if let Some(result) = self.search_state.results.get(result_index) {
            let target_line = result.line_index;
            let center_offset = (self.viewport_height / 2).max(1);
            self.scroll_offset = target_line.saturating_sub(center_offset);

            let max_scroll = self
                .content_length
                .saturating_sub(self.viewport_height.max(1));
            if self.scroll_offset > max_scroll {
                self.scroll_offset = max_scroll;
            }
        }
    }

    pub fn set_viewport_height(&mut self, height: u16) {
        self.viewport_height = height.saturating_sub(2) as usize;
    }

    fn exit_search_mode(&mut self) {
        self.mode = AppMode::Normal;
        // Keep search results active for navigation with n/N
    }

    fn cancel_search(&mut self) {
        self.mode = AppMode::Normal;
        self.clear_search();
    }

    fn clear_search(&mut self) {
        self.search_state = SearchState::default();
    }

    pub fn get_file_name(&self) -> &str {
        &self.file_name
    }

    pub fn get_rendered_content(&self) -> &Text<'static> {
        &self.rendered_content
    }

    pub fn get_scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub fn get_content_length(&self) -> usize {
        self.content_length
    }

    pub fn is_watching(&self) -> bool {
        self.watching
    }

    pub fn get_mode(&self) -> &AppMode {
        &self.mode
    }

    pub fn get_search_state(&self) -> &SearchState {
        &self.search_state
    }

    pub fn get_status_message(&self) -> Option<&str> {
        self.status_message.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::NamedTempFile;

    fn create_test_app() -> App {
        let temp_file = NamedTempFile::new().unwrap();
        fs::write(&temp_file, "# Test\n\nLine 1\nLine 2\nLine 3\nLine 4\nLine 5\nLine 6\nLine 7\nLine 8\nLine 9\nLine 10\nLine 11\nLine 12\nLine 13\nLine 14\nLine 15\nLine 16\nLine 17\nLine 18\nLine 19\nLine 20").unwrap();

        App::new(temp_file.path().to_path_buf(), false).unwrap()
    }

    #[test]
    fn test_scroll_to_top() {
        let mut app = create_test_app();
        app.scroll_offset = 10;
        app.scroll_to_top();
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn test_scroll_to_bottom() {
        let mut app = create_test_app();
        app.scroll_to_bottom();
        assert_eq!(app.scroll_offset, app.content_length.saturating_sub(1));
    }

    #[test]
    fn test_scroll_to_middle() {
        let mut app = create_test_app();
        app.scroll_to_middle();
        assert_eq!(app.scroll_offset, app.content_length / 2);
    }

    #[test]
    fn test_scroll_half_page_up() {
        let mut app = create_test_app();
        let half_page = (app.viewport_height / 2).max(1);
        app.scroll_offset = half_page + 3;
        app.scroll_half_page_up();
        assert_eq!(app.scroll_offset, 3);
    }

    #[test]
    fn test_scroll_half_page_down() {
        let mut app = create_test_app();
        let half_page = (app.viewport_height / 2).max(1);
        app.scroll_offset = 3;
        app.scroll_half_page_down();
        assert_eq!(app.scroll_offset, 3 + half_page);
    }

    #[test]
    fn test_scroll_bounds() {
        let mut app = create_test_app();

        // Test scrolling up from top
        app.scroll_offset = 0;
        app.scroll_up();
        assert_eq!(app.scroll_offset, 0);

        // Test scrolling down past bottom
        app.scroll_offset = app.content_length.saturating_sub(1);
        app.scroll_down();
        assert_eq!(app.scroll_offset, app.content_length.saturating_sub(1));
    }

    #[test]
    fn test_search_functionality() {
        let mut app = create_test_app();

        // Test starting search
        app.start_search();
        assert_eq!(app.mode, AppMode::Search);
        assert!(app.search_state.query.is_empty());
        assert!(app.search_state.results.is_empty());

        // Test search query
        app.search_state.query = "Line".to_string();
        app.perform_search();
        assert!(!app.search_state.results.is_empty());
        assert_eq!(app.search_state.current_result_index, Some(0));
    }

    #[test]
    fn test_search_navigation() {
        let mut app = create_test_app();

        // Setup search with multiple results
        app.search_state.query = "Line".to_string();
        app.perform_search();
        let result_count = app.search_state.results.len();

        // Test next result
        let initial_index = app.search_state.current_result_index.unwrap();
        app.next_search_result();
        assert_eq!(
            app.search_state.current_result_index,
            Some((initial_index + 1) % result_count)
        );

        // Test previous result
        app.previous_search_result();
        assert_eq!(app.search_state.current_result_index, Some(initial_index));
    }

    #[test]
    fn test_search_clear() {
        let mut app = create_test_app();

        // Setup search
        app.start_search();
        app.search_state.query = "test".to_string();
        app.perform_search();

        // Clear search
        app.clear_search();
        assert!(!app.search_state.is_active);
        assert!(app.search_state.query.is_empty());
        assert!(app.search_state.results.is_empty());
        assert_eq!(app.search_state.current_result_index, None);
    }
}
