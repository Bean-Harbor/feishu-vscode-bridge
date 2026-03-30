param(
    [string]$WorkspacePath,
    [string]$ApprovalRequired = "none",
    [string]$TargetDir = "target/bridge-live-runner",
    [switch]$SkipBuild,
    [switch]$NoEnv,
    [switch]$PrintOnly
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent $scriptDir

if (-not $WorkspacePath) {
    $WorkspacePath = $repoRoot
}

function Import-DotEnv {
    param([string]$Path)

    if (-not (Test-Path -LiteralPath $Path)) {
        return
    }

    Get-Content -LiteralPath $Path | ForEach-Object {
        $line = $_.Trim()
        if (-not $line -or $line.StartsWith("#")) {
            return
        }

        $separatorIndex = $line.IndexOf("=")
        if ($separatorIndex -lt 1) {
            return
        }

        $name = $line.Substring(0, $separatorIndex).Trim()
        $value = $line.Substring($separatorIndex + 1).Trim()

        if (($value.StartsWith('"') -and $value.EndsWith('"')) -or ($value.StartsWith("'") -and $value.EndsWith("'"))) {
            $value = $value.Substring(1, $value.Length - 2)
        }

        [System.Environment]::SetEnvironmentVariable($name, $value)
    }
}

Push-Location $repoRoot
try {
    if (-not $NoEnv) {
        Import-DotEnv (Join-Path $repoRoot ".env")
    }

    $env:BRIDGE_WORKSPACE_PATH = $WorkspacePath
    $env:BRIDGE_APPROVAL_REQUIRED = $ApprovalRequired
    $env:CARGO_TARGET_DIR = $TargetDir

    $binaryPath = Join-Path $repoRoot (Join-Path $TargetDir "debug/bridge-cli.exe")
    $commandPreview = @(
        "Set BRIDGE_WORKSPACE_PATH=$WorkspacePath",
        "Set BRIDGE_APPROVAL_REQUIRED=$ApprovalRequired",
        "Set CARGO_TARGET_DIR=$TargetDir",
        "cargo build --bin bridge-cli",
        "$binaryPath listen"
    ) -join [Environment]::NewLine

    if ($PrintOnly) {
        Write-Output $commandPreview
        return
    }

    Write-Output "Using workspace: $WorkspacePath"
    Write-Output "Using approval policy: $ApprovalRequired"
    Write-Output "Using cargo target dir: $TargetDir"

    if (-not $SkipBuild) {
        cargo build --bin bridge-cli
        if ($LASTEXITCODE -ne 0) {
            exit $LASTEXITCODE
        }
    }

    & $binaryPath listen
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}
finally {
    Pop-Location
}