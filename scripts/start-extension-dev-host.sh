#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"

workspace_path="$repo_root"
port="8766"
code_cli=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --workspace-path)
            workspace_path="$2"
            shift 2
            ;;
        --port)
            port="$2"
            shift 2
            ;;
        --code-cli)
            code_cli="$2"
            shift 2
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

if [[ -z "$code_cli" ]]; then
    if command -v code >/dev/null 2>&1; then
        code_cli="$(command -v code)"
    elif [[ -x "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code" ]]; then
        code_cli="/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code"
    elif [[ -x "$HOME/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code" ]]; then
        code_cli="$HOME/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code"
    else
        echo "Unable to find a usable VS Code CLI. Pass --code-cli <path>." >&2
        exit 1
    fi
fi

cd "$repo_root"

export BRIDGE_AGENT_BRIDGE_PORT="$port"
export BRIDGE_AGENT_BOOTSTRAP_WORKSPACE="$workspace_path"

echo "Using workspace: $workspace_path"
echo "Using agent bridge port: $port"
echo "Using VS Code CLI: $code_cli"

exec "$code_cli" --new-window "$workspace_path" --extensionDevelopmentPath="$repo_root/vscode-agent-bridge"