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

use std::env;

use ratatui::style::Color;
use syntect::highlighting::ThemeSet;

/// Detect whether the terminal uses a light background via `$COLORFGBG`.
///
/// The variable has the format `foreground;background` where `0` is black
/// and `7` is white. We only care about the background component: a value
/// of `7` means the terminal has a light background.
///
/// If the variable is unset or unparseable, we assume a dark terminal
/// (the more common default).
fn terminal_has_light_background() -> bool {
    env::var("COLORFGBG")
        .ok()
        .and_then(|v| {
            // Format is "fg;bg" – we only need the bg part.
            let bg = v.split(';').nth(1)?;
            Some(bg == "7")
        })
        .unwrap_or(false)
}

/// Return the built-in syntect theme matching the detected terminal brightness.
fn default_syntect_theme() -> syntect::highlighting::Theme {
    let themes = ThemeSet::load_defaults();
    if terminal_has_light_background() {
        themes.themes["Solarized (light)"].clone()
    } else {
        themes.themes["Solarized (dark)"].clone()
    }
}

/// UI colors and syntect theme for rendering markdown content.
#[derive(Debug, Clone)]
pub struct ThemeColors {
    pub heading_h1: Color,
    pub heading_h2: Color,
    pub heading_h3: Color,
    pub inline_code_fg: Color,
    pub inline_code_bg: Color,
    pub border: Color,
    pub table_header: Color,
    pub list_bullet: Color,
    pub image_placeholder: Color,
    /// Syntect theme for code block syntax highlighting.
    pub syntect_theme: syntect::highlighting::Theme,
}

impl ThemeColors {
    /// Return the heading color for the given level (1–6).
    /// Levels 4–6 fall back to the H3 color.
    pub fn heading_color(&self, level: u8) -> Color {
        match level {
            1 => self.heading_h1,
            2 => self.heading_h2,
            _ => self.heading_h3,
        }
    }
}

/// Theme selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, Default)]
pub enum Theme {
    /// Terminal-native colors from the user's palette.
    #[default]
    Index,
    /// Dracula theme with fixed RGB colors.
    Dracula,
    /// Solarized Dark theme with fixed RGB colors.
    SolarizedDark,
}

impl ThemeColors {
    /// Load colors from a named theme.
    pub fn from_theme(theme: Theme) -> Self {
        let syntect_theme = default_syntect_theme();

        match theme {
            Theme::Index => Self::index_colors(syntect_theme),
            Theme::Dracula => Self::dracula_colors(syntect_theme),
            Theme::SolarizedDark => Self::solarized_dark_colors(syntect_theme),
        }
    }

    /// Terminal-native colors using `Color::Indexed` so they adapt to
    /// the user's terminal palette.
    fn index_colors(syntect_theme: syntect::highlighting::Theme) -> Self {
        Self {
            heading_h1: Color::Indexed(5),        // terminal magenta / pink
            heading_h2: Color::Indexed(4),        // terminal blue / purple
            heading_h3: Color::Indexed(6),        // terminal cyan
            inline_code_fg: Color::Indexed(7),    // terminal white
            inline_code_bg: Color::Indexed(8),    // bright black / dark gray
            border: Color::Indexed(8),            // bright black / dark gray
            table_header: Color::Indexed(5),      // terminal magenta
            list_bullet: Color::Indexed(5),       // terminal magenta
            image_placeholder: Color::Indexed(6), // terminal cyan
            syntect_theme,
        }
    }

    /// Dracula theme with fixed RGB values.
    fn dracula_colors(syntect_theme: syntect::highlighting::Theme) -> Self {
        Self {
            heading_h1: Color::Rgb(255, 121, 198), // pink
            heading_h2: Color::Rgb(189, 147, 249), // purple
            heading_h3: Color::Rgb(139, 233, 253), // cyan
            inline_code_fg: Color::Rgb(207, 207, 194),
            inline_code_bg: Color::Rgb(68, 71, 90),
            border: Color::Rgb(98, 114, 164),
            table_header: Color::Rgb(255, 121, 198),
            list_bullet: Color::Rgb(255, 121, 198),
            image_placeholder: Color::Rgb(98, 114, 164),
            syntect_theme,
        }
    }

    /// Solarized Dark theme with fixed RGB values.
    fn solarized_dark_colors(syntect_theme: syntect::highlighting::Theme) -> Self {
        Self {
            heading_h1: Color::Rgb(220, 50, 47),          // red
            heading_h2: Color::Rgb(211, 54, 130),         // magenta
            heading_h3: Color::Rgb(42, 161, 152),         // cyan
            inline_code_fg: Color::Rgb(238, 232, 213),    // base2
            inline_code_bg: Color::Rgb(70, 75, 90),       // base01
            border: Color::Rgb(101, 123, 131),            // base01
            table_header: Color::Rgb(211, 54, 130),       // magenta
            list_bullet: Color::Rgb(211, 54, 130),        // magenta
            image_placeholder: Color::Rgb(101, 185, 202), // cyan
            syntect_theme,
        }
    }
}
