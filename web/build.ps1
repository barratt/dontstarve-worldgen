# Builds the WebAssembly generator (ds-worldgen-web.js + ds_worldgen_web.wasm).
# Requires the Emscripten SDK (https://emscripten.org) and the Rust target:
#   rustup target add wasm32-unknown-emscripten
#   ./web/build.ps1 -Emsdk C:\path\to\emsdk [-OutDir web]
# A game data bundle named by $env:DSW_EMBED is built in (see
# build-standalone.ps1); without it the page asks for the game folder.
param([string]$Emsdk = $env:EMSDK, [string]$OutDir = "")
$ErrorActionPreference = "Stop"
if (-not $Emsdk) { throw "Pass -Emsdk <emsdk directory> or set EMSDK" }
$ems = Join-Path $Emsdk "upstream\emscripten"
if (-not (Test-Path "$ems\emcc.exe") -and -not (Test-Path "$ems\emcc.bat")) { throw "emcc not found under $ems (run: emsdk install latest; emsdk activate latest)" }
$ext = if (Test-Path "$ems\emcc.exe") { "exe" } else { "bat" }
$env:EMSDK = $Emsdk
$env:EM_CONFIG = Join-Path $Emsdk ".emscripten"
$env:PATH = "$ems;$env:PATH"
# cc-rs and rustc default to emcc.bat; point them at the launchers emsdk ships.
$env:CC_wasm32_unknown_emscripten = "$ems\emcc.$ext"
$env:CXX_wasm32_unknown_emscripten = "$ems\em++.$ext"
$env:AR_wasm32_unknown_emscripten = "$ems\emar.$ext"
$env:CARGO_TARGET_WASM32_UNKNOWN_EMSCRIPTEN_LINKER = "$ems\emcc.$ext"
$root = Split-Path -Parent $PSScriptRoot
if (-not $OutDir) { $OutDir = Join-Path $root "web" }
Push-Location $root
try {
    # Panic messages embed source paths; map the local Cargo home and checkout
    # to neutral prefixes so the published module carries no local paths.
    $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $HOME ".cargo" }
    $remap = "target.wasm32-unknown-emscripten.rustflags=['--remap-path-prefix=$cargoHome=/cargo', '--remap-path-prefix=$root=/dsworldgen']"
    cargo build --release --target wasm32-unknown-emscripten --features web --bin ds-worldgen-web --config $remap
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    $out = "target/wasm32-unknown-emscripten/release"
    New-Item -ItemType Directory -Force $OutDir | Out-Null
    Copy-Item "$out/ds-worldgen-web.js" -Destination $OutDir
    Copy-Item "$out/ds_worldgen_web.wasm" -Destination $OutDir
    Get-ChildItem $OutDir -Filter "ds*web*" | Select-Object Name, Length
}
finally { Pop-Location }
