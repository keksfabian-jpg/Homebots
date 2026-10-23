# HomeBots test runner (for Claude's checks on this PC): builds once, then runs each scenario
# with a fixed-step recording (HB_RECORD) and writes the frames to logs\rec_<name>\.
# Usage: powershell -ExecutionPolicy Bypass -File tests\hbtest.ps1 [scenario ...]
# Without arguments all scenarios in tests\scenarios.txt run.
# scenarios.txt: one line per scenario: <name> | <fps,from,to> | VAR=value VAR=value ...
$ErrorActionPreference = "Continue"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$logs = Join-Path $root "logs"
New-Item -ItemType Directory -Force $logs | Out-Null
$status = Join-Path $logs "test_status.txt"
"building $(Get-Date -Format s)" | Set-Content $status
# cargo writes progress to stderr: run it through cmd so PowerShell does not treat that as errors
cmd /c "cargo build --release -p homebots > `"$logs\test_build.txt`" 2>&1"
if ($LASTEXITCODE -ne 0) { "build FAILED $(Get-Date -Format s)" | Set-Content $status; exit 1 }
$exe = Join-Path $root "target\release\homebots.exe"
# tests\only.txt (optional, one name per line): run only these scenarios when no names are given
$only = @()
$onlyFile = Join-Path $PSScriptRoot "only.txt"
if ($args.Count -eq 0 -and (Test-Path $onlyFile)) { $only = @(Get-Content $onlyFile | ForEach-Object { $_.Trim() } | Where-Object { $_ }) }
$lines = Get-Content (Join-Path $PSScriptRoot "scenarios.txt") | Where-Object { $_.Trim() -and -not $_.Trim().StartsWith("#") }
foreach ($line in $lines) {
    $parts = $line.Split("|") | ForEach-Object { $_.Trim() }
    $name = $parts[0]; $rec = $parts[1]; $vars = if ($parts.Count -gt 2) { $parts[2] } else { "" }
    if ($args.Count -gt 0 -and -not ($args -contains $name)) { continue }
    if ($only.Count -gt 0 -and -not ($only -contains $name)) { continue }
    "running $name $(Get-Date -Format s)" | Set-Content $status
    $dir = Join-Path $logs "rec_$name"
    if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
    New-Item -ItemType Directory -Force $dir | Out-Null
    # environment for this run only
    $set = @{
        "HB_RECORD" = "$dir,$rec"
        "HOMEBOTS_MENU_OPEN" = "U_OnlineMenuScreen,PC_U_CreateMPGameScreen"
        "HOMEBOTS_MENU_LAUNCH" = "1"
        "HOMEBOTS_AUTOSPAWN" = "1"
        "HOMEBOTS_SIZE" = "1280x720"
        "HOMEBOTS_LANG" = "deu"
    }
    foreach ($kv in ($vars -split "\s+" | Where-Object { $_ })) {
        $i = $kv.IndexOf("="); $set[$kv.Substring(0, $i)] = $kv.Substring($i + 1)
    }
    foreach ($k in $set.Keys) { [Environment]::SetEnvironmentVariable($k, $set[$k], "Process") }
    $p = Start-Process -FilePath $exe -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $logs "test_$name.log") -RedirectStandardError (Join-Path $logs "test_$name.err")
    if (-not $p.WaitForExit(600000)) { Stop-Process -Id $p.Id; "timeout $name" | Add-Content $status }
    foreach ($k in $set.Keys) { [Environment]::SetEnvironmentVariable($k, $null, "Process") }
}
"done $(Get-Date -Format s)" | Set-Content $status
