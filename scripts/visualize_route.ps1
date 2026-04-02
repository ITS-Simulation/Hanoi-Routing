<#
.SYNOPSIS
  Mở bản đồ trực quan hóa kết quả truy vấn route CCH-Hanoi.
.DESCRIPTION
  Nếu không truyền file:  mở HTML, kéo thả .geojson để xem.
  Nếu truyền file:        khởi HTTP server nhỏ + tự load file lên bản đồ.
.EXAMPLE
  .\scripts\visualize_route.ps1
  .\scripts\visualize_route.ps1 query_2026-04-02T100314.geojson
  .\scripts\visualize_route.ps1 *.geojson
#>
param(
    [Parameter(ValueFromRemainingArguments)]
    [string[]]$Files
)

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$HtmlFile  = Join-Path $ScriptDir "visualize_route.html"

if (-not (Test-Path $HtmlFile)) {
    Write-Error "visualize_route.html not found at $HtmlFile"
    exit 1
}

if ($Files -and $Files.Count -gt 0) {
    # Resolve glob patterns
    $ResolvedFiles = @()
    foreach ($f in $Files) {
        $ResolvedFiles += Get-Item $f -ErrorAction SilentlyContinue
    }
    if ($ResolvedFiles.Count -eq 0) {
        Write-Error "No valid .geojson files found"
        exit 1
    }

    # Temporary directory
    $TmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("route_viz_" + (Get-Random))
    New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null

    Copy-Item $HtmlFile (Join-Path $TmpDir "visualize_route.html")

    $FileNames = @()
    foreach ($f in $ResolvedFiles) {
        Copy-Item $f.FullName (Join-Path $TmpDir $f.Name)
        $FileNames += $f.Name
    }

    $FileList = $FileNames -join ","

    # Inject auto-load script
    $InjectScript = @"
<script>
(function() {
  var files = '$FileList'.split(',');
  files.forEach(function(f) {
    if (!f) return;
    fetch(f)
      .then(function(r) { return r.json(); })
      .then(function(geojson) { addRoute(geojson, f); })
      .catch(function(e) { console.warn('Auto-load failed:', f, e); });
  });
})();
</script></body>
"@

    $HtmlContent = Get-Content (Join-Path $TmpDir "visualize_route.html") -Raw
    $HtmlContent = $HtmlContent -replace '</body>', $InjectScript
    Set-Content -Path (Join-Path $TmpDir "visualize_route.html") -Value $HtmlContent -Encoding UTF8

    $Port = Get-Random -Minimum 8100 -Maximum 8200
    Write-Host "Starting HTTP server on port $Port ..." -ForegroundColor Cyan
    Write-Host "Open: http://localhost:$Port/visualize_route.html" -ForegroundColor Green
    Write-Host "Press Ctrl+C to stop.`n"

    Start-Process "http://localhost:$Port/visualize_route.html"

    try {
        Push-Location $TmpDir
        python -m http.server $Port --bind 127.0.0.1
    }
    finally {
        Pop-Location
        Remove-Item -Recurse -Force $TmpDir -ErrorAction SilentlyContinue
    }
}
else {
    Start-Process $HtmlFile
    Write-Host "Opened visualize_route.html" -ForegroundColor Green
    Write-Host "Drag & drop .geojson files onto the map to view routes."
}
