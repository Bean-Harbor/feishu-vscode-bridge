param(
    [string]$Configuration = "Release",
    [string]$OutputDir = "dist/windows"
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$outputRoot = Join-Path $repoRoot $OutputDir
$payloadRoot = Join-Path $outputRoot "payload"
$nsisScript = Join-Path $PSScriptRoot "windows-installer.nsi"

Write-Host "[windows-installer] repo root: $repoRoot"
Write-Host "[windows-installer] output dir: $outputRoot"

New-Item -ItemType Directory -Force -Path $payloadRoot | Out-Null

Write-Host "[windows-installer] building Rust binaries"
if ($Configuration -ieq "Release") {
    cargo build --bin bridge-cli --bin setup-gui --release
    $targetDir = "release"
} else {
    cargo build --bin bridge-cli --bin setup-gui
    $targetDir = "debug"
}
$bridgeCli = Join-Path $repoRoot "target/$targetDir/bridge-cli.exe"
$setupGui = Join-Path $repoRoot "target/$targetDir/setup-gui.exe"

if (-not (Test-Path $bridgeCli)) {
    throw "bridge-cli.exe not found at $bridgeCli"
}

if (-not (Test-Path $setupGui)) {
    throw "setup-gui.exe not found at $setupGui"
}

Copy-Item $bridgeCli (Join-Path $payloadRoot "bridge-cli.exe") -Force
Copy-Item $setupGui (Join-Path $payloadRoot "setup-gui.exe") -Force

$vsixCandidates = @(
    (Join-Path $repoRoot "vscode-agent-bridge/feishu-agent-bridge.vsix"),
    (Join-Path $repoRoot "vscode-agent-bridge/dist/feishu-agent-bridge.vsix")
)

foreach ($candidate in $vsixCandidates) {
    if (Test-Path $candidate) {
        Copy-Item $candidate (Join-Path $payloadRoot "feishu-agent-bridge.vsix") -Force
        break
    }
}

if (-not (Get-Command makensis -ErrorAction SilentlyContinue)) {
    Write-Warning "makensis not found. Payload prepared at $payloadRoot, but Setup.exe was not generated yet."
    exit 0
}

if (-not (Test-Path $nsisScript)) {
    Write-Warning "NSIS script not found at $nsisScript. Payload prepared at $payloadRoot, but Setup.exe was not generated yet."
    exit 0
}

Write-Host "[windows-installer] building NSIS installer"
makensis /DOUTPUT_DIR="$outputRoot" /DPAYLOAD_DIR="$payloadRoot" $nsisScript
Write-Host "[windows-installer] done"