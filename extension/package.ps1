# Builds store-ready upload bundles (no Developer Mode required once published on
# Chrome Web Store, Microsoft Edge Add-ons, and Mozilla Add-ons).
$ErrorActionPreference = "Stop"

$extDir = $PSScriptRoot
$outDir = Join-Path (Split-Path $extDir -Parent) "dist"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

$stageRoot = Join-Path $outDir "_stage"
if (Test-Path $stageRoot) { Remove-Item -Recurse -Force $stageRoot }

function Copy-ExtensionFiles($targetDir) {
    New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
    Get-ChildItem -Path $extDir -Exclude "package.ps1" | Copy-Item -Destination $targetDir -Recurse -Force
}

# 1. Chromium bundle (Chrome Web Store / Edge Add-ons / Brave / Opera / Vivaldi)
$chromiumStage = Join-Path $stageRoot "chromium"
Copy-ExtensionFiles $chromiumStage
$chromiumZip = Join-Path $outDir "lajufile-extension-chromium.zip"
if (Test-Path $chromiumZip) { Remove-Item -Force $chromiumZip }
Compress-Archive -Path "$chromiumStage\*" -DestinationPath $chromiumZip

# 2. Firefox AMO bundle (rewrites MV3 service_worker -> background.scripts + gecko id)
$firefoxStage = Join-Path $stageRoot "firefox"
Copy-ExtensionFiles $firefoxStage
$manifestPath = Join-Path $firefoxStage "manifest.json"
$manifest = Get-Content -Raw $manifestPath | ConvertFrom-Json
$manifest.background = @{ scripts = @("background.js") }
$manifest | Add-Member -NotePropertyName "browser_specific_settings" -NotePropertyValue @{
    gecko = @{
        id = "lajufile@lajufile.app"
        strict_min_version = "128.0"
    }
} -Force
$manifest | ConvertTo-Json -Depth 10 | Set-Content -Path $manifestPath -Encoding UTF8
$firefoxZip = Join-Path $outDir "lajufile-extension-firefox.zip"
if (Test-Path $firefoxZip) { Remove-Item -Force $firefoxZip }
Compress-Archive -Path "$firefoxStage\*" -DestinationPath $firefoxZip

Remove-Item -Recurse -Force $stageRoot
Get-Item $chromiumZip, $firefoxZip | Select-Object FullName, Length
