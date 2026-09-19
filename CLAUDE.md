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

- **hawk-terminal** (main): GUI application using Iced - handles windows, panes, user interaction
- **data** (`hawk-data`): Data structures, state management, configuration, layout persistence
- **exchange** (`hawk-exchange`): Exchange adapters for Binance, Bybit, Hyperliquid, OKX

### Key Patterns

**State Management**: `SavedState` serializes to `saved-state.json` in user data directory. Layout configurations use UUID-based IDs with nested Split/Leaf pane hierarchies.

**Event Flow**: All app events flow through a `Message` enum. Market data arrives via `exchange::Event` from WebSocket streams. Each chart pane maintains independent state.

**Exchange Abstraction**: The `Adapter` trait abstracts exchange differences. Uses `fastwebsockets` for real-time data and `reqwest` for REST APIs with rate limiting.

**Data Aggregation**: Supports time-based (candlesticks) and tick-based (footprints) aggregation across multiple chart types: Heatmap, Kline, Footprint, Ladder, Time & Sales, Comparison.

### Code Style

- Line width: 100 characters (rustfmt.toml)
- Clippy thresholds: 16 function args, 5 enum variants (clippy.toml)
- Uses `rustc-hash` (FxHashMap) and `sonic-rs` for performance
