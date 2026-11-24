<#
.SYNOPSIS
  Grow the spec tree by one level: expand every open node at the shallowest open
  level with a coding agent (in parallel), run the level check, then validate.

.EXAMPLE
  .\tools\grow.ps1 -DryRun
  .\tools\grow.ps1 -Parallel 4
  .\tools\grow.ps1 -Level 2 -Limit 5 -Agent codex

.NOTES
  Splits at levels 1 and 2 require independent agent review before merging (AGENTS.md rule 8, D-052).
  This script never commits or pushes. Review the diff, then open a PR.
  Windows PowerShell 5.1 compatible.
#>
param(
  [int]$Level = -1,
  [int]$Limit = 0,
  [int]$Parallel = 3,
  [ValidateSet("claude", "codex")]
  [string]$Agent = "claude",
  [string]$Model = "",
  [switch]$DryRun,
  [switch]$NoLevelCheck
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

$scopeArgs = @("tools/scope.py", "open")
if ($Level -ge 0) { $scopeArgs += @("--level", "$Level") }
if ($Limit -gt 0) { $scopeArgs += @("--limit", "$Limit") }
$nodes = @(& python @scopeArgs | Where-Object { $_ -and $_.Trim() })

if ($nodes.Count -eq 0) {
  Write-Host "No open nodes. The tree is fully expanded (or blocked on decisions)."
  exit 0
}

$targetLevel = (($nodes[0] -replace '^spec/?', '') -split '/').Count - 1
Write-Host "Level $targetLevel : $($nodes.Count) open node(s) to expand with '$Agent' (parallel $Parallel)."
$nodes | ForEach-Object { Write-Host "  $_" }
if ($DryRun) { exit 0 }

$logDir = Join-Path $Root "generated/logs"
New-Item -ItemType Directory -Force $logDir | Out-Null

function Get-AgentCommand([string]$message) {
  if ($Agent -eq "claude") {
    $a = @("-p", $message, "--permission-mode", "acceptEdits",
           "--allowedTools", "Read,Write,Edit,Glob,Grep,Bash(python tools/scope.py:*)")
    if ($Model) { $a += @("--model", $Model) }
    return @{ Exe = "claude"; Args = $a }
  }
  return @{ Exe = "codex"; Args = @("exec", "--full-auto", $message) }
}

$agentJob = {
  # The agent's own arguments arrive as the remaining positional values in $args.
  param($root, $exe, $log)
  $argv = [string[]]$args
  Set-Location $root
  & $exe @argv *> $log
  "exit=$LASTEXITCODE" | Add-Content $log
}

function Start-AgentJob([string]$message, [string]$log) {
  $cmd = Get-AgentCommand $message
  $jobArgs = @($Root, $cmd.Exe, $log) + [string[]]$cmd.Args
  return Start-Job -ScriptBlock $agentJob -ArgumentList $jobArgs
}

function Test-AgentLog([string]$log) {
  $text = Get-Content $log -Raw -ErrorAction SilentlyContinue
  if ($text -match 'Failed to authenticate|OAuth access token is invalid|not logged in|Invalid API key') {
    Write-Host "Agent authentication failed (see $log). Sign in with '$Agent' interactively, then rerun." -ForegroundColor Red
    return $false
  }
  return $true
}

# Canary: run the first node alone, so a broken setup fails fast instead of 17 times.
$jobs = @()
$first = $true
foreach ($node in $nodes) {
  while (@($jobs | Where-Object { $_.State -eq "Running" }).Count -ge $Parallel) {
    Wait-Job -Job $jobs -Any | Out-Null
  }
  $log = Join-Path $logDir (($node -replace '[/\\]', '_') + ".log")
  Write-Host "-> expanding $node"
  $job = Start-AgentJob "Follow prompts/expand-node.md exactly. TARGET: $node" $log
  $jobs += $job
  if ($first) {
    $first = $false
    Wait-Job -Job $job | Out-Null
    if (-not (Test-AgentLog $log)) { $jobs | Remove-Job -Force; exit 2 }
  }
}

Wait-Job -Job $jobs | Out-Null
$jobs | Remove-Job
Write-Host "All expansions finished. Logs: generated/logs/"

if (-not $NoLevelCheck) {
  $nextLevel = $targetLevel + 1
  Write-Host "-> level check for level $nextLevel"
  $log = Join-Path $logDir "level-check-$nextLevel.log"
  $job = Start-AgentJob "Follow prompts/level-check.md exactly. LEVEL: $nextLevel" $log
  Wait-Job -Job $job | Out-Null
  Remove-Job $job
  [void](Test-AgentLog $log)
}

& python tools/scope.py check
exit $LASTEXITCODE
