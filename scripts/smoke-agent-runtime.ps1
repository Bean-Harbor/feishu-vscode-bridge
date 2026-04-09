param(
    [string]$BridgeUrl = "http://127.0.0.1:8765",
    [string]$WorkspacePath,
    [string]$PlanPrompt = "__DEFAULT_GIT_SYNC_PROMPT__",
    [string]$AgentPrompt = "Analyze what the parse_intent function does. If the current context is not enough, read the relevant code first, then answer with a conclusion and next-step suggestion.",
    [string]$ContinuePrompt = "Continue and give me the smallest next-step suggestion. If needed, read the most relevant code before answering.",
    [int]$TimeoutSec = 180,
    [switch]$IncludeAgent,
    [switch]$Json
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent $scriptDir

if (-not $WorkspacePath) {
    $WorkspacePath = $repoRoot
}

$defaultPlanPromptJson = '\u628a\u672c\u5730\u6539\u52a8\u540c\u6b65\u5230github\u4e0a'

function Write-Step {
    param([string]$Message)
    Write-Output "==> $Message"
}

function Fail {
    param([string]$Message)
    throw $Message
}

function Assert-Condition {
    param(
        [bool]$Condition,
        [string]$Message
    )

    if (-not $Condition) {
        Fail $Message
    }
}

function Invoke-BridgeJson {
    param(
        [string]$Path,
        [hashtable]$Payload
    )

    $uri = "$BridgeUrl$Path"
    $body = $Payload | ConvertTo-Json -Depth 20
    Invoke-RestMethod -Uri $uri -Method Post -ContentType "application/json" -Body $body -TimeoutSec $TimeoutSec
}

function Invoke-BridgeJsonBody {
    param(
        [string]$Path,
        [string]$Body
    )

    $uri = "$BridgeUrl$Path"
    Invoke-RestMethod -Uri $uri -Method Post -ContentType "application/json" -Body $Body -TimeoutSec $TimeoutSec
}

function Get-ExceptionMessage {
    param([System.Exception]$Exception)

    if ($null -ne $Exception.ErrorDetails -and -not [string]::IsNullOrWhiteSpace($Exception.ErrorDetails.Message)) {
        return $Exception.ErrorDetails.Message
    }

    return $Exception.Message
}

function Invoke-BridgeJsonSafe {
    param(
        [string]$Path,
        [hashtable]$Payload
    )

    try {
        $response = Invoke-BridgeJson -Path $Path -Payload $Payload
        return [pscustomobject]@{
            success = $true
            response = $response
            error = $null
        }
    } catch {
        return [pscustomobject]@{
            success = $false
            response = $null
            error = (Get-ExceptionMessage -Exception $_.Exception)
        }
    }
}

function Test-AgentModelAvailabilityBlock {
    param([string]$Message)

    if ([string]::IsNullOrWhiteSpace($Message)) {
        return $false
    }

    $patterns = @(
        "premium model quota",
        "premium requests",
        "model unavailable",
        "provider unavailable",
        "no compatible chat model",
        "allowance to renew",
        "language model",
        "could not obtain a valid planning decision",
        "failed to plan the next autonomous step"
    )

    foreach ($pattern in $patterns) {
        if ($Message.ToLowerInvariant().Contains($pattern.ToLowerInvariant())) {
            return $true
        }
    }

    return $false
}

function Summarize-Options {
    param($Options)

    @($Options | ForEach-Object {
        [pscustomobject]@{
            label = $_.label
            command = $_.command
        }
    })
}

function Get-AgentBlockedMessage {
    param($Response)

    if ($null -eq $Response) {
        return $null
    }

    $parts = @()
    if ($null -ne $Response.message) {
        $parts += [string]$Response.message
    }
    if ($null -ne $Response.error) {
        $parts += [string]$Response.error
    }
    if ($null -ne $Response.run) {
        if ($null -ne $Response.run.summary) {
            $parts += [string]$Response.run.summary
        }
        if ($null -ne $Response.run.currentAction) {
            $parts += [string]$Response.run.currentAction
        }
    }

    $combined = ($parts | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }) -join "`n"
    if (Test-AgentModelAvailabilityBlock -Message $combined) {
        return $combined
    }

    return $null
}

Write-Step "Checking bridge health"
$health = Invoke-RestMethod -Uri "$BridgeUrl/health" -Method Get -TimeoutSec 10
Assert-Condition ($health.status -eq "ok") "Bridge health check failed: status=$($health.status)"

$sessionId = "smoke-agent-$(New-Guid)"

Write-Step "Validating semantic planner confirm flow"
$planPayload = @{
    sessionId = $sessionId
    prompt = $PlanPrompt
    currentProject = $WorkspacePath
}
$planBody = $planPayload | ConvertTo-Json -Depth 20
if ($PlanPrompt -eq "__DEFAULT_GIT_SYNC_PROMPT__") {
    $planBody = $planBody.Replace('"' + $PlanPrompt + '"', '"' + $defaultPlanPromptJson + '"')
}
$plan = Invoke-BridgeJsonBody -Path "/v1/chat/plan" -Body $planBody

Assert-Condition ($plan.decision -eq "confirm") "Expected semantic planner decision=confirm, got '$($plan.decision)'"
Assert-Condition (@($plan.options).Count -ge 2) "Expected at least 2 confirmation options, got $(@($plan.options).Count)"

$validOptions = @($plan.options | Where-Object {
    -not [string]::IsNullOrWhiteSpace($_.label) -and -not [string]::IsNullOrWhiteSpace($_.command)
})
Assert-Condition (@($validOptions).Count -eq @($plan.options).Count) "Semantic planner returned confirmation options with an empty label or command"

$optionCommands = @($validOptions | ForEach-Object { $_.command.Trim() })
$distinctOptionCommands = @($optionCommands | Sort-Object -Unique)
Assert-Condition (@($distinctOptionCommands).Count -ge 2) "Semantic planner confirmation options did not contain at least 2 distinct commands"

$hasDirectPullOnlyRegression = $distinctOptionCommands | Where-Object {
    $_ -match '^(?i)git\s+pull$'
}
Assert-Condition (@($hasDirectPullOnlyRegression).Count -eq 0) "Semantic planner regressed to a direct git pull option for an ambiguous GitHub sync request"

$gitRelatedOptions = @($validOptions | Where-Object {
    ($_.label -match '(?i)git|github|push|pull|commit|repo|仓库|同步|状态') -or
    ($_.command -match '(?i)git|github|push|pull|commit|repo|仓库|同步|状态')
})
Assert-Condition (@($gitRelatedOptions).Count -eq @($validOptions).Count) "Semantic planner confirmation options included a non-git candidate for an ambiguous GitHub sync request"

$safeStatusOption = @($validOptions | Where-Object {
    $_.command.Trim() -match '^(?i)同步\s+git\s+状态$'
})
Assert-Condition (@($safeStatusOption).Count -ge 1) "Semantic planner confirmation options did not include the safe status-check command for an ambiguous GitHub sync request"

$result = [pscustomobject]@{
    health = [pscustomobject]@{
        status = $health.status
        port = $health.port
        sessions = $health.sessions
    }
    planner = [pscustomobject]@{
        decision = $plan.decision
        summaryForUser = $plan.summaryForUser
        risk = $plan.risk
        optionCount = @($plan.options).Count
        options = @(Summarize-Options -Options $plan.options)
    }
    agent = $null
}

if ($IncludeAgent) {
    Write-Step "Starting autonomous agent runtime"
    $start = Invoke-BridgeJsonSafe -Path "/v1/chat/agent/start" -Payload @{
        sessionId = $sessionId
        prompt = $AgentPrompt
        currentProject = $WorkspacePath
    }

    if (-not $start.success) {
        $blockedByModel = Test-AgentModelAvailabilityBlock -Message $start.error
        $result.agent = [pscustomobject]@{
            included = $true
            blocked = $blockedByModel
            blockedReason = if ($blockedByModel) { "model_availability" } else { $null }
            message = if ($blockedByModel) { "Agent smoke blocked by model availability." } else { "Agent smoke failed during start." }
            error = $start.error
        }

        if (-not $blockedByModel) {
            Fail "Agent smoke failed during start: $($start.error)"
        }
    } else {
        Assert-Condition ($null -ne $start.response.run) "Agent start did not return run state"
        Assert-Condition (-not [string]::IsNullOrWhiteSpace($start.response.run.runId)) "Agent start did not return run id"
        $startBlockedMessage = Get-AgentBlockedMessage -Response $start.response
        if ($start.response.run.status -in @("failed", "cancelled") -and $null -ne $startBlockedMessage) {
            $result.agent = [pscustomobject]@{
                included = $true
                blocked = $true
                blockedReason = "model_availability"
                sessionId = $sessionId
                runId = $start.response.run.runId
                startStatus = $start.response.run.status
                message = "Agent smoke blocked by model availability during start."
                error = $startBlockedMessage
            }
        } else {
            Assert-Condition ($start.response.run.status -notin @("failed", "cancelled")) "Agent start returned terminal failure status '$($start.response.run.status)'"
        }

        if ($null -ne $result.agent -and $result.agent.blocked) {
            if ($Json) {
                $result | ConvertTo-Json -Depth 20
                exit 0
            }

            Write-Output ""
            Write-Output "Smoke OK (planner)"
            Write-Output "health: $($result.health.status) on port $($result.health.port)"
            Write-Output "planner: decision=$($result.planner.decision), risk=$($result.planner.risk), options=$($result.planner.optionCount)"
            foreach ($option in $result.planner.options) {
                Write-Output "  - $($option.label) => $($option.command)"
            }
            Write-Output "Agent check blocked by model availability"
            Write-Output "  reason: $($result.agent.blockedReason)"
            Write-Output "  detail: $($result.agent.error)"
            exit 0
        }

        Assert-Condition (@($start.response.run.checkpoints).Count -gt 0) "Agent start returned no checkpoints"

        $startCheckpointCount = @($start.response.run.checkpoints).Count

        Write-Step "Continuing autonomous agent runtime"
        $continue = Invoke-BridgeJsonSafe -Path "/v1/chat/agent/continue" -Payload @{
            sessionId = $sessionId
            runId = $start.response.run.runId
            prompt = $ContinuePrompt
        }

        if (-not $continue.success) {
            $blockedByModel = Test-AgentModelAvailabilityBlock -Message $continue.error
            $result.agent = [pscustomobject]@{
                included = $true
                blocked = $blockedByModel
                blockedReason = if ($blockedByModel) { "model_availability" } else { $null }
                sessionId = $sessionId
                runId = $start.response.run.runId
                startStatus = $start.response.run.status
                startCheckpointCount = $startCheckpointCount
                message = if ($blockedByModel) { "Agent smoke blocked by model availability during continue." } else { "Agent smoke failed during continue." }
                error = $continue.error
            }

            if (-not $blockedByModel) {
                Fail "Agent smoke failed during continue: $($continue.error)"
            }
        } else {
            Assert-Condition ($null -ne $continue.response.run) "Agent continue did not return run state"
            Assert-Condition ($continue.response.run.runId -eq $start.response.run.runId) "Agent continue returned a different run id"
            $continueBlockedMessage = Get-AgentBlockedMessage -Response $continue.response
            if ($continue.response.run.status -in @("failed", "cancelled") -and $null -ne $continueBlockedMessage) {
                $result.agent = [pscustomobject]@{
                    included = $true
                    blocked = $true
                    blockedReason = "model_availability"
                    sessionId = $sessionId
                    runId = $start.response.run.runId
                    startStatus = $start.response.run.status
                    continueStatus = $continue.response.run.status
                    startCheckpointCount = $startCheckpointCount
                    continueCheckpointCount = @($continue.response.run.checkpoints).Count
                    message = "Agent smoke blocked by model availability during continue."
                    error = $continueBlockedMessage
                }
            } else {
                Assert-Condition ($continue.response.run.status -notin @("failed", "cancelled")) "Agent continue returned terminal failure status '$($continue.response.run.status)'"
            }

            if ($null -ne $result.agent -and $result.agent.blocked) {
                continue
            }

            Assert-Condition (@($continue.response.run.checkpoints).Count -gt $startCheckpointCount) "Agent continue did not add new checkpoints"

            $result.agent = [pscustomobject]@{
                included = $true
                blocked = $false
                blockedReason = $null
                sessionId = $sessionId
                runId = $start.response.run.runId
                startStatus = $start.response.run.status
                continueStatus = $continue.response.run.status
                startCheckpointCount = $startCheckpointCount
                continueCheckpointCount = @($continue.response.run.checkpoints).Count
                startCurrentAction = $start.response.run.currentAction
                continueCurrentAction = $continue.response.run.currentAction
                startNextAction = $start.response.run.nextAction
                continueNextAction = $continue.response.run.nextAction
                message = "Agent smoke passed."
                error = $null
            }
        }
    }
}

if ($Json) {
    $result | ConvertTo-Json -Depth 20
} else {
    Write-Output ""
    Write-Output "Smoke OK (planner)"
    Write-Output "health: $($result.health.status) on port $($result.health.port)"
    Write-Output "planner: decision=$($result.planner.decision), risk=$($result.planner.risk), options=$($result.planner.optionCount)"
    foreach ($option in $result.planner.options) {
        Write-Output "  - $($option.label) => $($option.command)"
    }
    if ($IncludeAgent) {
        if ($result.agent.blocked) {
            Write-Output "Agent check blocked by model availability"
            Write-Output "  reason: $($result.agent.blockedReason)"
            Write-Output "  detail: $($result.agent.error)"
        } elseif ($null -ne $result.agent) {
            Write-Output "Agent check passed"
            Write-Output "  run: $($result.agent.runId)"
            Write-Output "  start: status=$($result.agent.startStatus), checkpoints=$($result.agent.startCheckpointCount)"
            Write-Output "  continue: status=$($result.agent.continueStatus), checkpoints=$($result.agent.continueCheckpointCount)"
        }
    }
}
