# mdless

A terminal-based markdown file viewer built with Rust and ratatui. See [README.md](README.md) for project overview, features, and usage.

## Build & Test Commands

```bash
cargo build                                          # build
cargo test --all-features                            # run tests
cargo fmt --all                                      # format
cargo clippy --all-targets --all-features -- -D warnings  # lint
cargo check --all-targets --all-features             # fast type check
cargo audit                                          # security audit
```

## Architecture

- `main.rs` — CLI entry point via clap; parses args and hands off to `App`
- `app.rs` — `App` struct owns all runtime state (scroll offset, search state, file watcher); drives the event loop and delegates rendering to `ui`
- `markdown.rs` — `MarkdownRenderer` parses markdown with pulldown-cmark and converts it to ratatui `Text`; handles syntax highlighting via syntect and draws bordered code blocks
- `ui.rs` — stateless drawing functions; renders header, scrollable content, and a footer/search bar based on current `AppMode`
- `error.rs` — `MdViewError` enum (IO, file-watch, terminal) and a project-wide `Result<T>` alias

## Dependencies

- `ratatui` + `crossterm` — TUI framework and terminal backend
- `pulldown-cmark` — markdown parsing
- `syntect` — syntax highlighting inside code blocks (theme: `base16-ocean.dark`)
- `notify` — file system watching for `-w` (watch) mode
- `clap` — CLI argument parsing
- `anyhow` / `thiserror` — error handling (`thiserror` for typed errors in the library, `anyhow` available for application-level use)

## Rust Style

### Naming
- `snake_case` for variables, functions, modules
- `PascalCase` for structs, enums, traits, type aliases
- `SCREAMING_SNAKE_CASE` for constants and statics
- `is_` / `has_` prefix for boolean functions

### Error Handling
- Use `Result<T, E>` with the `?` operator for propagation
- Use `anyhow` for application errors, `thiserror` for library errors
- No `.unwrap()` in library code; panics only where truly unrecoverable

### Ownership
- Prefer borrowing (`&T`, `&[T]`) over cloning
- Use `Cow<str>` for conditionally-owned strings
- Use `Arc<T>` / `Mutex<T>` for shared state across threads

### Idioms
- Prefer iterator chains (`.filter().map().collect()`) over manual loops
- Prefer `match` for multi-arm patterns; `if let` for single-arm
- Pre-allocate collections with `Vec::with_capacity(n)` when size is known
- Use generics over trait objects (`dyn Trait`) when dispatch is compile-time

## Git Conventions

### Branch Names
`feature/<description>`, `fix/<description>`, `docs/<description>`

### Commit Format (Conventional Commits)
```
<type>[optional scope]: <description>

[optional body — explain what and why, not how]
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`

Rules:
- Imperative mood: "Add feature" not "Added feature"
- Subject line ≤ 50 characters, no trailing period
- Body wrapped at 72 characters

### Workflow
- Rebase feature branches onto main before merging
- Squash commits when merging to main
- Never commit secrets, API keys, or credentials
