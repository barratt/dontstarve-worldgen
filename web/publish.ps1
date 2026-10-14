# Builds the public site (build-standalone.ps1 -Public) and deploys it to
# GitHub Pages without committing it: the site is attached to a temporary
# release, the Pages workflow deploys it, and the release is deleted again.
#
#   ./web/publish.ps1 -Emsdk C:\path\to\emsdk `
#       -Game "C:\Program Files (x86)\Steam\steamapps\common\dont_starve" `
#       -Dst  "C:\Program Files (x86)\Steam\steamapps\common\Don't Starve Together"
#   ./web/publish.ps1 -NoBuild        # deploy the existing dist-public/
#
# Requires the GitHub CLI (gh), signed in with access to the repository.
param(
    [string]$Emsdk = $env:EMSDK,
    [string]$Game = "",
    [string]$Dst = "",
    [switch]$NoBuild
)
# gh reports progress on stderr, which Windows PowerShell would treat as an
# error under "Stop"; failures are checked through $LASTEXITCODE instead.
$ErrorActionPreference = "Continue"
$root = Split-Path -Parent $PSScriptRoot
$site = Join-Path $root "dist-public"
Push-Location $root
try {
    if (-not $NoBuild) {
        & "$PSScriptRoot\build-standalone.ps1" -Public -Emsdk $Emsdk -Game $Game -Dst $Dst -OutDir $site
    }
    if (-not (Test-Path "$site\ds_worldgen_web.wasm")) { throw "$site has no build; run without -NoBuild" }
    $zip = Join-Path $root "target\site.zip"
    if (Test-Path $zip) { Remove-Item $zip }
    Compress-Archive -Path "$site\*" -DestinationPath $zip

    # A temporary release carries the site to the workflow.
    gh release delete site --yes --cleanup-tag 2>&1 | Out-Null
    gh release create site $zip --title "Site upload (temporary)" --notes "Deployed to Pages by web/publish.ps1, then removed." --prerelease
    if ($LASTEXITCODE -ne 0) { throw "could not create the release" }
    try {
        gh workflow run pages.yml
        if ($LASTEXITCODE -ne 0) { throw "could not start the Pages workflow" }
        Start-Sleep -Seconds 5
        $run = gh run list --workflow pages.yml --limit 1 --json databaseId --jq ".[0].databaseId"
        gh run watch $run --exit-status
        if ($LASTEXITCODE -ne 0) { throw "the Pages deploy failed (gh run view $run --log-failed)" }
    }
    finally {
        gh release delete site --yes --cleanup-tag 2>&1 | Out-Null
    }
    $url = gh api "repos/{owner}/{repo}/pages" --jq ".html_url"
    Write-Host "Published: $url"
}
finally { Pop-Location }
