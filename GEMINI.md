# GEMINI.md

Guidance and architectural constraints for Google Gemini / Antigravity when working with the Hawk Terminal codebase.

## Critical Invariants (DO NOT VIOLATE)

1. **Trade Storage & Cache Currency Invariant**:
   - `SizeUnit::Base = 0` is the canonical default and hard storage invariant.
   - Binary tick caches (`.bin`, `.fst`), in-memory footprints, cumulative deltas, and aggregations MUST ALWAYS store and compute raw quantities in **Base units** (token/coin count, e.g. BTC, ETH).
   - NEVER serialize Quote-multiplied quantities (`qty * price`) into historical trade caches.
   - Currency conversion to `Quote` (USD/USDT) is strictly allowed ONLY dynamically at the presentation layer (UI drawing / chart labels).

2. **User Data & Installer Isolation**:
   - The installer (`installer.iss`) MUST NEVER copy, deploy, or overwrite mutable state files into `{userappdata}` (`saved-state.json`, `drawings.json`, `alerts.json`, etc.).
   - Initial state and profiles are created at runtime by `SavedState::default()` or migrated safely from legacy folders.

3. **Legacy Migration Invariant**:
   - `data_path()` in `data/src/lib.rs` executes `ensure_migration_from_legacy()`. If `%APPDATA%\hawk-terminal` is absent or holds empty stubs (`<= 2` bytes), it automatically migrates existing layouts, drawings, journals, and cache from `%APPDATA%\flowsurface`.
   - Never remove or bypass this fallback logic.

4. **Heatmap Renderer Stability**:
   - CPU Canvas Heatmap (`HeatmapChart`) is the stable primary renderer.
   - Experimental WGPU `ShaderHeatmap` is disabled/hidden from default panel selectors due to platform driver instabilities.

---

## Workspace Structure

- **hawk-terminal** (root crate): GUI desktop application built on `Iced 0.14` (MVU Elm-like pattern), managing panes, windows, inputs, and drawing overlays.
- **data** (`hawk-data`): In-memory time series, footprint buckets, tick caches, layout configurations (`SavedState`), alerts, drawings, and local file I/O.
- **exchange** (`hawk-exchange`): WebSocket and REST connectors with rate limiting for Binance, Bybit, Hyperliquid, OKX, and MEXC.
- **indicator-engine**: Scripted indicators engine, CLI tools, and deterministic catalog generator (`catalog.json` with SHA-256).

---

## Build & Test Commands

```powershell
# Check compilation across all crates
cargo check --workspace

# Build release executable
cargo build --release

# Run full test suite (570+ unit and integration tests)
cargo test --workspace

# Run targeted package tests
cargo test -p hawk-data -p hawk-exchange

# Clippy linting (must pass with zero warnings)
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Format check
cargo fmt --all -- --check
```

---

## Roadmap & Task Management

- **[ROADMAP.md](ROADMAP.md)**: Single source of truth for active tasks, architectural decisions, and current release progress. Keep high information density and update the task matrix.
- **[ROADMAP_ARCHIVE.md](ROADMAP_ARCHIVE.md)**: Archive of 110+ completed historical tickets. Do not bloat active `ROADMAP.md` with closed legacy tasks.
