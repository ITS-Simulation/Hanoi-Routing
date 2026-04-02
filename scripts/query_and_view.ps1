<#
.SYNOPSIS
  Chạy truy vấn CCH-Hanoi rồi tự động mở kết quả trên bản đồ.
.EXAMPLE
  .\scripts\query_and_view.ps1 --from-lat 21.0283 --from-lng 105.8542 --to-lat 20.998 --to-lng 105.829
.EXAMPLE
  $env:PROFILE="car"; .\scripts\query_and_view.ps1 --from-lat 21.03 --from-lng 105.85 --to-lat 21.01 --to-lng 105.83
#>
param(
    [Parameter(ValueFromRemainingArguments)]
    [string[]]$QueryArgs
)

$ErrorActionPreference = "Stop"

$Repo      = "C:\ITS\Routing\Hanoi-Routing"
$Profile   = if ($env:PROFILE) { $env:PROFILE } else { "motorcycle" }
$DataDir   = if ($env:DATA_DIR) { $env:DATA_DIR } else { "$Repo\Maps\data\hanoi_$Profile" }
$CliBin    = if ($env:CLI_BIN) { $env:CLI_BIN } else { "$Repo\CCH-Hanoi\target\release\cch-hanoi.exe" }
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Visualizer = Join-Path $ScriptDir "visualize_route.ps1"

$Timestamp  = Get-Date -Format "yyyy-MM-ddTHHmmss"
$OutputFile = Join-Path $Repo "query_$Timestamp.geojson"

Write-Host "=== CCH-Hanoi Query + Visualize ===" -ForegroundColor Cyan
Write-Host "Profile:  $Profile"
Write-Host "Data dir: $DataDir"
Write-Host ""

# Check if CLI is available — fall back to WSL if no Windows binary
$UseWsl = $false
if (-not (Test-Path $CliBin)) {
    $WslCli = "/mnt/c/ITS/Routing/Hanoi-Routing/CCH-Hanoi/target/release/cch-hanoi"
    Write-Host "Windows binary not found, using WSL..." -ForegroundColor Yellow
    $UseWsl = $true
}

$WslDataDir  = $DataDir -replace '^C:', '/mnt/c' -replace '\\', '/'
$WslOutput   = $OutputFile -replace '^C:', '/mnt/c' -replace '\\', '/'

$AllArgs = @(
    "query",
    "--data-dir", $(if ($UseWsl) { $WslDataDir } else { $DataDir }),
    "--line-graph",
    "--output-format", "geojson",
    "--demo",
    "--output-file", $(if ($UseWsl) { $WslOutput } else { $OutputFile })
) + $QueryArgs

if ($UseWsl) {
    $LdPath = "/mnt/c/ITS/Routing/Hanoi-Routing/RoutingKit/lib"
    $EnvCmd = "export LD_LIBRARY_PATH='$LdPath':$`{LD_LIBRARY_PATH:-}; source ~/.cargo/env 2>/dev/null || true;"
    $ArgsStr = ($AllArgs | ForEach-Object { "'$_'" }) -join " "
    wsl bash -c "$EnvCmd $WslCli $ArgsStr"
} else {
    & $CliBin @AllArgs
}

if ($LASTEXITCODE -ne 0) {
    Write-Error "Query failed (exit $LASTEXITCODE)"
    exit $LASTEXITCODE
}

Write-Host ""
Write-Host "Output: $OutputFile" -ForegroundColor Green

if ($env:NO_VIEW -eq "1") {
    Write-Host "Skipping visualization (NO_VIEW=1)"
} else {
    Write-Host "Opening visualizer..."
    & $Visualizer $OutputFile
}
