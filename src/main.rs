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

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

mod app;
mod error;
mod markdown;
mod ui;

use app::App;

#[derive(Parser)]
#[command(name = "mdless")]
#[command(about = "A terminal-based markdown file viewer")]
#[command(version)]
struct Cli {
    /// Path to the markdown file to view
    file: PathBuf,

    /// Enable file watching for live updates
    #[arg(short, long)]
    watch: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let mut app = App::new(cli.file, cli.watch)?;
    app.run()?;

    Ok(())
}
