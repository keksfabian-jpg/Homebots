# HomeBots release package: builds the release exe and puts it with the project's own assets
# into dist\HomeBots (and dist\HomeBots.zip). Game files are NOT copied: HomeBots reads them from
# the installed Homefront (default Steam path; another path goes into gamepath.txt).
$ErrorActionPreference = "Continue"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
New-Item -ItemType Directory -Force (Join-Path $root "logs") | Out-Null
$log = Join-Path $root "logs\release_build.txt"
"building $(Get-Date -Format s)" | Set-Content $log
cmd /c "cargo build --release -p homebots >> `"$log`" 2>&1"
if ($LASTEXITCODE -ne 0) { "build FAILED" | Add-Content $log; Write-Host "Build failed, see logs\release_build.txt"; exit 1 }
$dist = Join-Path $root "dist\HomeBots"
if (Test-Path $dist) { Remove-Item -Recurse -Force $dist }
New-Item -ItemType Directory -Force $dist | Out-Null
Copy-Item (Join-Path $root "target\release\homebots.exe") (Join-Path $dist "HomeBots.exe")
Copy-Item -Recurse (Join-Path $root "assets") (Join-Path $dist "assets")
Copy-Item (Join-Path $PSScriptRoot "README.txt") (Join-Path $dist "README.txt")
@"
# Path to the Homefront installation (only needed if it is not in the default Steam path).
# One line without # at the start, e.g.:
# D:\SteamLibrary\steamapps\common\HOMEFRONT
"@ | Set-Content -Encoding UTF8 (Join-Path $dist "gamepath.txt")
$zip = Join-Path $root "dist\HomeBots.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path $dist -DestinationPath $zip
"done $(Get-Date -Format s)" | Add-Content $log
Write-Host "Done: dist\HomeBots\HomeBots.exe and dist\HomeBots.zip"
