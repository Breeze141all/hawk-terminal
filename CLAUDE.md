# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Run Commands

```bash
# Build release
cargo build --release

# Run application
cargo run --release

# Build with hot-reload (debug feature)
cargo build --features debug
cargo run --features debug

# Include the experimental WGPU heatmap (hidden from the UI)
cargo build --features shader-heatmap

# Tests (use --no-fail-fast to see every failing suite, not just the first)
cargo test --workspace --no-fail-fast

# Also run the #[ignore]d tests that hit live exchanges (CI skips them)
cargo test --workspace --no-fail-fast -- --include-ignored

# Lint (must pass with no warnings)
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Format check
cargo fmt --all -- --check --verbose

# Auto-format
cargo fmt --all
```

## System Dependencies

- **Linux**: `sudo apt install build-essential pkg-config libasound2-dev libfontconfig1-dev libxcb-render0-dev`
- **macOS**: `xcode-select --install`
- **Windows**: None required

## Architecture

Hawk Terminal is a desktop crypto charting application built with Rust and the Iced GUI framework (Elm-inspired MVU architecture).

### Workspace Structure

- **hawk-terminal** (main): GUI application using Iced - handles windows, panes, user interaction, in-app updater (`src/updater.rs`)
- **data** (`hawk-data`): Data structures, state management, configuration, layout persistence
- **exchange** (`hawk-exchange`): Exchange adapters for Binance, Bybit, Hyperliquid, OKX, MEXC, Coinbase (plus the Bitcoin Counterflow open-interest source and API trade-history import)
- **indicator-engine**: Scripted indicator language (parser, compiler, runtime), CLI tools and the deterministic `catalog.json` generator

### Key Patterns

**State Management**: `SavedState` serializes to `saved-state.json` in user data directory. Layout configurations use UUID-based IDs with nested Split/Leaf pane hierarchies.

**Event Flow**: All app events flow through a `Message` enum. Market data arrives via `exchange::Event` from WebSocket streams. Each chart pane maintains independent state.

**Exchange Abstraction**: The `Adapter` trait abstracts exchange differences. Uses `fastwebsockets` for real-time data and `reqwest` for REST APIs with rate limiting.

**Data Aggregation**: Supports time-based (candlesticks) and tick-based (footprints) aggregation across multiple chart types: Heatmap, Kline, Footprint, Ladder, Time & Sales, Comparison.

### Code Style

- Line width: 100 characters (rustfmt.toml)
- Clippy thresholds: 16 function args, 5 enum variants (clippy.toml)
- Uses `rustc-hash` (FxHashMap) and `sonic-rs` for performance

## Critical Invariants (DO NOT VIOLATE)

1. **Trade cache currency**: `SizeUnit::Base = 0`. Tick caches (`.bin`, `.fst`), footprints and CVD are stored and computed in Base units (coin count). Never serialize `qty * price`; convert to Quote only at the presentation layer.
2. **Installer isolation**: `installer.iss` must never write into `{userappdata}` (`saved-state.json`, `drawings.json`, `alerts.json`, ...).
3. **Legacy migration**: keep `ensure_migration_from_legacy` in `data/src/lib.rs` (migrates `%APPDATA%\flowsurface` to `hawk-terminal`).
4. **Heatmap renderer**: the CPU Canvas heatmap is the default; the experimental WGPU `ShaderHeatmap` stays hidden and is compiled only with the `shader-heatmap` cargo feature (off by default; `--all-features` in CI still builds it). Without the feature, saved or requested `ShaderHeatmap` panes open as the canvas heatmap.
5. **Updater trust**: `updater::download_installer_file` must keep refusing releases without a valid `sha256`, URLs outside the project's GitHub release/site paths, and checksum mismatches. `release.yaml` must keep writing `sha256` into `version.json`.
6. **Secrets**: exchange API keys/secrets for trade import live in memory only. Never persist or log them; `ImportCredentials` has a redacting `Debug` impl, keep it.

## Conventions

- Persist JSON through `data::write_json_to_file` (atomic, fsynced). Load lists through `load_json_list`: unparsable files are moved to `*_corrupt_<ts>` rather than silently replaced by defaults.
- Do not `unwrap()` on data from the network or disk; return `AdapterError::ParseError` (the stream loops ignore `Err`).
- Tests must not depend on the current date or hardcode "today"; test production functions, not copies of their logic.
- A test that reaches a live exchange gets `#[ignore = "network: ..."]`. CI (`test.yml`, and the `test` job that gates every build in `release.yaml`) runs `cargo test --workspace --locked` without them.
- Line endings are LF everywhere, enforced by `.gitattributes` (`* text=auto eol=lf`). Do not write CRLF; if an editor does, `git add --renormalize .` fixes it. The one-off normalization commit is listed in `.git-blame-ignore-revs`.
- `cargo fmt --check` is not clean on `main` today (pre-existing); format only the files you touch.
