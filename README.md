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

## GUI Setup Wizard

This repository also includes a desktop setup wizard for guiding users through VS Code detection and Feishu bot configuration.

Run it with:

```bash
cargo run --bin setup-gui
```

The setup wizard currently supports:

- Windows
- macOS
- Linux

What it does:

- Detects whether VS Code is installed before continuing
- Guides the user to install VS Code if it is missing
- Lets the user open the current project in VS Code or open the project directory
- Collects Feishu webhook settings and writes them to `.env`

Compatibility status:

- Windows: locally compiled and launched
- macOS: code path implemented and checked in CI with `cargo check --bin setup-gui`
- Linux: code path implemented and checked in CI with `cargo check --bin setup-gui`

## Current Scope

- Core command and plan execution engine (Rust)
- `继续` / `执行全部` intent support
- Minimal CLI demo executor
- Native desktop setup GUI for initial configuration

## Next Milestones

- Feishu adapter as independent module
- VS Code bridge action adapter
- Session persistence and approval gates
- End-to-end integration tests

## Why This Repo

The original implementation lived in a larger HarborNAS workspace with nested repositories. This project isolates the bridge capability for open-source adoption and easier contribution.

## License

MIT
