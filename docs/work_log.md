# Work Log

## 2026-03-26

### Summary

- Added a native desktop setup wizard for `feishu-vscode-bridge`
- Implemented VS Code detection before Feishu configuration
- Added guided installation flow when VS Code is missing
- Added actions to open the current workspace in VS Code or open the project directory
- Improved cross-platform support for Windows, macOS, and Linux
- Added CI checks for `setup-gui` on Ubuntu, Windows, and macOS

### Files Updated

- `Cargo.toml`
- `README.md`
- `.github/workflows/ci.yml`
- `src/bin/setup_gui.rs`

### Verification

- Local Windows compile check passed: `cargo check --bin setup-gui`
- Local Windows launch smoke test passed: `cargo run --bin setup-gui`
- GitHub Actions now includes multi-platform compile validation for the setup GUI

### Notes

- macOS detection now covers both `/Applications/Visual Studio Code.app` and `~/Applications/Visual Studio Code.app`
- The setup wizard writes Feishu configuration to `.env` in the project root

### Next Candidates

- Preserve unrelated existing environment variables when updating `.env`
- Validate Feishu webhook format before saving
- Add screenshots for the setup wizard to the README