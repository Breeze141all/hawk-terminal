# Hawk Terminal

[![Rust](https://img.shields.io/badge/Rust-2024_Edition-black.svg?style=flat&logo=rust)](https://www.rust-lang.org)
[![Iced](https://img.shields.io/badge/GUI-Iced_0.14-white.svg?style=flat)](https://github.com/iced-rs/iced)
[![License](https://img.shields.io/badge/License-GPL--3.0-blue.svg)](LICENSE)
[![Website](https://img.shields.io/badge/Website-hawk--site-zinc.svg)](https://breeze141all.github.io/hawk-site/)

A native desktop charting and order flow platform for cryptocurrency markets, written in Rust on [Iced](https://github.com/iced-rs/iced) and WGPU. No Electron, no garbage collector.

Hawk Terminal is a fork of [flowsurface](https://github.com/flowsurface-rs/flowsurface) by akenshaw, which provides the core charting, heatmap, footprint and exchange stream foundation. This fork adds the features listed below on top of it. See [Credits](#credits).

---

## Key Modules & Capabilities

- **Estimated Liquidation Heatmap**: A model of where positions opened on each candle at 5x–100x leverage would be liquidated, weighted by that candle's volume. It does not use open interest or real liquidation events, so treat the zones as an estimate.
- **Footprint & Cluster Analysis**: Tick-level volume profile, delta, cumulative volume delta (CVD), and imbalance detection.
- **Multi-Period Market Profile & TPO**: 30-minute letter brackets, Initial Balance (IB), Value Area 70% (VAH/VAL), and Point of Control (POC).
- **Tick-Level Market Replay**: Session replay from a local binary LZ4 trade cache, so replays run offline.
- **Direct Exchange Connectors** (public market data straight from each exchange's WebSocket/REST API):
  - Binance (Spot, USD-M and COIN-M Futures)
  - Bybit (Spot, Linear and Inverse Perpetuals)
  - Hyperliquid (Spot & Perpetuals)
  - OKX (Spot, Swap & Futures)
  - MEXC (Spot, Linear and Inverse Futures)
  - Coinbase (Spot)
- **Local-First, No Analytics**: Layouts, drawings, alerts, and journal notes are stored only on your disk (plain files, not encrypted). Exchange API keys used for trade import are kept in memory only and are never written to disk. The only network traffic is exchange market data, optional trade import, and an update check against the project site.

---

## Supported Platforms

| Platform | Architecture | Build Toolchain |
| :--- | :--- | :--- |
| **Windows** | x86_64 | MSVC (`stable-x86_64-pc-windows-msvc`) |
| **macOS** | Apple Silicon & Intel | Xcode Command Line Tools |
| **Linux** | x86_64 | `build-essential pkg-config libasound2-dev libfontconfig1-dev libxcb-render0-dev` |

---

## Build & Installation

### 1. Prerequisites (Install Rust)
Ensure you have the latest Rust toolchain installed:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### 2. Clone Repository
```bash
git clone https://github.com/Breeze141all/hawk-terminal.git
cd hawk-terminal
```

### 3. Build & Run (Release)
```bash
# Build optimized release binary
cargo build --release

# Run terminal
cargo run --release

# Run the tests (CI runs the same on every push and pull request)
cargo test --workspace
```

Binary will be compiled directly to `target/release/hawk-terminal`.

---

## Architecture Overview

Hawk Terminal uses a multi-crate Cargo workspace architecture:
- **`hawk-terminal`** (`src/`): Application entry point, Elm-inspired MVU GUI event loop via Iced, layout orchestration, canvas painters.
- **`hawk-data`** (`data/`): Domain models, binary LZ4 tick caching, indicator math, session clusters, state serialization.
- **`hawk-exchange`** (`exchange/`): WebSocket streams (`fastwebsockets`), REST clients and normalized adapters for the six exchanges above, plus API trade-history import.
- **`indicator-engine`** (`indicator-engine/`): the scripted indicator language (parser, compiler, runtime), its CLI tools and the `catalog.json` generator.

---

## Credits
Hawk Terminal started as a fork of [flowsurface](https://github.com/flowsurface-rs/flowsurface) by [akenshaw](https://github.com/akenshaw) and keeps its git history. The charting engine, the original heatmap/footprint/kline/ladder panels and the first Binance, Bybit, Hyperliquid and OKX adapters come from flowsurface; upstream fixes are still ported (the roadmap marks them with `Джерело: Flowsurface <commit>`). If you only need those core features, consider using and supporting the upstream project.

---

## License
Distributed under the GNU General Public License v3.0 or later (GPL-3.0-or-later), the same license as flowsurface. See [LICENSE](LICENSE) for details.
