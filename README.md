# Feishu VS Code Bridge

A standalone open-source Rust project for bridging Feishu commands to local VS Code development actions.

This repository extracts the Feishu <-> VS Code bridge flow from HarborNAS work and focuses on:

- Intent parsing for chat commands
- Plan execution with step-by-step mode
- Continue-all mode (`执行全部`) for one-shot execution
- Safe pause-on-failure behavior

## Quick Start

```bash
cargo test
cargo run --bin bridge-cli -- 执行全部
```

## Current Scope

- Core command and plan execution engine (Rust)
- `继续` / `执行全部` intent support
- Minimal CLI demo executor

## Next Milestones

- Feishu adapter as independent module
- VS Code bridge action adapter
- Session persistence and approval gates
- End-to-end integration tests

## Why This Repo

The original implementation lived in a larger HarborNAS workspace with nested repositories. This project isolates the bridge capability for open-source adoption and easier contribution.

## License

MIT
