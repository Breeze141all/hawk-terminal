# Hawk Terminal

[![Rust](https://img.shields.io/badge/Rust-2024_Edition-black.svg?style=flat&logo=rust)](https://www.rust-lang.org)
[![Iced](https://img.shields.io/badge/GUI-Iced_0.14-white.svg?style=flat)](https://github.com/iced-rs/iced)
[![License](https://img.shields.io/badge/License-GPL--3.0-blue.svg)](LICENSE)
[![Website](https://img.shields.io/badge/Website-hawk--site-zinc.svg)](https://breeze141all.github.io/hawk-site/)

A sovereign, native desktop charting and order flow intelligence platform for cryptocurrency markets. Built entirely in bare-metal Rust with zero Electron and zero garbage collection pauses.

---

## Key Modules & Capabilities

- **2D Liquidation Heatmap**: Real-time calculated liquidation leverage bands (10x–100x) from raw exchange streams.
- **Footprint & Cluster Analysis**: Tick-level volume profile, delta, cumulative volume delta (CVD), and imbalance detection.
- **Multi-Period Market Profile & TPO**: 30-minute letter brackets, Initial Balance (IB), Value Area 70% (VAH/VAL), and Point of Control (POC).
- **Tick-Level Market Replay**: High-fidelity session replay with binary LZ4 cache for zero-latency testing.
- **Direct Sovereign Exchange Connectors**:
  - Binance (Spot & USD-M Futures)
  - Bybit (Linear Perpetuals & Spot)
  - Hyperliquid (Perpetuals L1/L2)
  - OKX (Swap & Futures)
- **Zero-Telemetry Security**: All API keys, layouts, and journal notes stay 100% encrypted on local disk.

---

## Supported Platforms

| Platform | Architecture | Build Toolchain |
| :--- | :--- | :--- |
| **Windows** | x86_64 | MSVC (`stable-x86_64-pc-windows-msvc`) |
| **macOS** | Apple Silicon & Intel | Xcode Command Line Tools |
| **Linux** | x86_64 | `build-essential pkg-config libasound2-dev` |

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
```

Binary will be compiled directly to `target/release/hawk-terminal`.

---

## Architecture Overview

Hawk Terminal uses a multi-crate Cargo workspace architecture:
- **`hawk-terminal`** (`src/`): Application entry point, Elm-inspired MVU GUI event loop via Iced, layout orchestration, canvas painters.
- **`hawk-data`** (`data/`): Domain models, binary LZ4 tick caching, indicator math, session clusters, state serialization.
- **`hawk-exchange`** (`exchange/`): Low-latency WebSocket actors (`fastwebsockets`), REST clients, normalized exchange adapters.

---

## License
Distributed under the GNU General Public License v3.0 or later (GPL-3.0-or-later). See [LICENSE](LICENSE) for details.
