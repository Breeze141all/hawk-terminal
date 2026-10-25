# AGENTS.md

Guidance and architectural constraints for AI Coding Agents (Antigravity, Claude, Codex, Cursor) working with Hawk Terminal.

Please refer to and strictly follow the invariants defined in:
- [GEMINI.md](file:///c:/Users/Breeze/Documents/hawk-workspace/hawk-client/GEMINI.md)
- [CLAUDE.md](file:///c:/Users/Breeze/Documents/hawk-workspace/hawk-client/CLAUDE.md)
- [ROADMAP.md](file:///c:/Users/Breeze/Documents/hawk-workspace/hawk-client/ROADMAP.md)

## Core Invariants Summary

1. **Trade Cache Currency**: Strictly `SizeUnit::Base = 0`. Never multiply `qty * price` into disk cache files (`.bin`, `.fst`).
2. **State & Installer Isolation**: `installer.iss` must never install files into `{userappdata}`.
3. **Legacy Migration**: Always preserve `ensure_migration_from_legacy` in `data/src/lib.rs`.
4. **Renderer**: Stable Canvas Heatmap is default; WGPU experimental Heatmap is disabled.
