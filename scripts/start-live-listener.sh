#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"

workspace_path="${BRIDGE_WORKSPACE_PATH:-$repo_root}"
project_mappings="${BRIDGE_PROJECT_MAPPINGS:-}"
approval_required="${BRIDGE_APPROVAL_REQUIRED:-none}"
target_dir="${CARGO_TARGET_DIR:-target/bridge-live-runner}"
skip_build=0
no_env=0
print_only=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --workspace-path)
            workspace_path="$2"
            shift 2
            ;;
        --approval-required)
            approval_required="$2"
            shift 2
            ;;
        --project-mappings)
            project_mappings="$2"
            shift 2
            ;;
        --target-dir)
            target_dir="$2"
            shift 2
            ;;
        --skip-build)
            skip_build=1
            shift
            ;;
        --no-env)
            no_env=1
            shift
            ;;
        --print-only)
            print_only=1
            shift
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

cd "$repo_root"

if [[ $no_env -eq 0 && -f .env ]]; then
    set -a
    . ./.env
    set +a
fi

export BRIDGE_WORKSPACE_PATH="$workspace_path"
if [[ -n "$project_mappings" ]]; then
    export BRIDGE_PROJECT_MAPPINGS="$project_mappings"
fi
export BRIDGE_APPROVAL_REQUIRED="$approval_required"
export CARGO_TARGET_DIR="$target_dir"

binary_path="$repo_root/$target_dir/debug/bridge-cli"

if [[ $print_only -eq 1 ]]; then
    printf 'export BRIDGE_WORKSPACE_PATH=%q\n' "$workspace_path"
    if [[ -n "$project_mappings" ]]; then
        printf 'export BRIDGE_PROJECT_MAPPINGS=%q\n' "$project_mappings"
    fi
    printf 'export BRIDGE_APPROVAL_REQUIRED=%q\n' "$approval_required"
    printf 'export CARGO_TARGET_DIR=%q\n' "$target_dir"
    echo "cargo build --bin bridge-cli"
    printf '%q listen\n' "$binary_path"
    exit 0
fi

echo "Using workspace: $workspace_path"
if [[ -n "$project_mappings" ]]; then
    echo "Using project mappings: $project_mappings"
fi
echo "Using approval policy: $approval_required"
echo "Using cargo target dir: $target_dir"

if [[ $skip_build -eq 0 ]]; then
    cargo build --bin bridge-cli
fi

exec "$binary_path" listen