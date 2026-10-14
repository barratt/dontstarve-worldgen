# Builds a standalone copy of the web page with the game data built in, from
# YOUR installs: no game folder needs to be chosen in the browser.
#
#   ./web/build-standalone.ps1 -Emsdk C:\path\to\emsdk `
#       -Game "C:\Program Files (x86)\Steam\steamapps\common\dont_starve" `
#       -Dst  "C:\Program Files (x86)\Steam\steamapps\common\Don't Starve Together" `
#       [-OutDir dist]
#
# Either -Game or -Dst may be left out. The result in dist/ contains the
# games' scripts (compressed, in the data bundle inside the .wasm): keep it
# local, do not commit it.
#
# -Public builds the version for hosting (dist-public/, or -OutDir): only the
# scripts world generation loads, compiled to stripped bytecode for the
# WebAssembly build, plus the icons and logos.
# Serve it with any static server, e.g.  python -m http.server -d dist 8000
param(
    [string]$Emsdk = $env:EMSDK,
    [string]$Game = "",
    [string]$Dst = "",
    [string]$OutDir = "",
    [switch]$Public
)
$ErrorActionPreference = "Stop"
if (-not $Game -and -not $Dst) { throw "Pass -Game <dont_starve dir> and/or -Dst <Don't Starve Together dir>" }
$root = Split-Path -Parent $PSScriptRoot
if (-not $OutDir) { $OutDir = Join-Path $root $(if ($Public) { "dist-public" } else { "dist" }) }
Push-Location $root
try {
    # 1. Collect the data world generation reads into one bundle.
    cargo build --release --bin ds-worldgen
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    $bundle = Join-Path $root "target\standalone\game.dswb"
    New-Item -ItemType Directory -Force (Split-Path $bundle) | Out-Null
    $packArgs = @("pack", "--output", $bundle)
    if ($Public) { $packArgs += "--worldgen-only" }
    if ($Game) { $packArgs += @("--game", $Game) }
    if ($Dst) { $packArgs += @("--dst", $Dst) }
    & ".\target\release\ds-worldgen.exe" @packArgs
    if ($LASTEXITCODE -ne 0) { throw "pack failed" }

    # 2. Build the WebAssembly module with the bundle embedded, next to the page.
    New-Item -ItemType Directory -Force $OutDir | Out-Null
    Copy-Item "web\index.html", "web\app.js", "web\worker.js", "web\style.css" -Destination $OutDir
    $env:DSW_EMBED = $bundle
    try { & "$PSScriptRoot\build.ps1" -Emsdk $Emsdk -OutDir $OutDir }
    finally { Remove-Item Env:\DSW_EMBED }
    Write-Host "Standalone page in $OutDir (keep it local; it contains game scripts)."
}
finally { Pop-Location }
