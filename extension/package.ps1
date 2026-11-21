# Builds store-ready upload bundles (no Developer Mode required once published on
# Microsoft Edge Add-ons, Mozilla Add-ons, and Chromium browsers).
$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

$extDir = $PSScriptRoot
$outDir = Join-Path (Split-Path $extDir -Parent) "dist"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

$stageRoot = Join-Path $outDir "_stage"
if (Test-Path $stageRoot) { Remove-Item -Recurse -Force $stageRoot }

function Copy-ExtensionFiles($targetDir) {
    New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
    Get-ChildItem -Path $extDir -Exclude "package.ps1" | Copy-Item -Destination $targetDir -Recurse -Force
}

function Write-ExtensionZip([string]$sourceDir, [string]$zipPath) {
    if (Test-Path $zipPath) { Remove-Item -Force $zipPath }
    $baseLen = ([System.IO.Path]::GetFullPath($sourceDir)).TrimEnd('\', '/').Length + 1
    $archive = [System.IO.Compression.ZipFile]::Open($zipPath, [System.IO.Compression.ZipArchiveMode]::Create)
    try {
        $files = Get-ChildItem -Path $sourceDir -File -Recurse | Sort-Object FullName
        foreach ($file in $files) {
            # Use POSIX '/' separators so Linux-hosted Edge/AMO validators unpack nested paths cleanly.
            $relPath = ([System.IO.Path]::GetFullPath($file.FullName)).Substring($baseLen).Replace('\', '/')
            [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
                $archive,
                $file.FullName,
                $relPath,
                [System.IO.Compression.CompressionLevel]::Optimal
            ) | Out-Null
        }
    } finally {
        $archive.Dispose()
    }
}

# 1. Chromium / Edge bundle (Microsoft Edge Add-ons / Brave / Opera / Vivaldi)
$chromiumStage = Join-Path $stageRoot "chromium"
Copy-ExtensionFiles $chromiumStage
$chromiumZip = Join-Path $outDir "lajufile-extension-chromium.zip"
Write-ExtensionZip $chromiumStage $chromiumZip

# 2. Firefox AMO bundle (background.scripts, Firefox 140+ data_collection_permissions, no Chromium keys)
$firefoxStage = Join-Path $stageRoot "firefox"
Copy-ExtensionFiles $firefoxStage
$manifestPath = Join-Path $firefoxStage "manifest.json"
$manifest = Get-Content -Raw $manifestPath | ConvertFrom-Json
$manifest.PSObject.Properties.Remove("minimum_chrome_version") | Out-Null
$manifest.description = "Send downloads, PDFs, and YouTube videos from Firefox to LajuFile Desktop on Windows, macOS, and Linux."
$manifest.background = @{ scripts = @("common.js", "background.js") }
$manifest | Add-Member -NotePropertyName "browser_specific_settings" -NotePropertyValue @{
    gecko = @{
        id = "lajufile@lajufile.app"
        strict_min_version = "140.0"
        data_collection_permissions = @{
            required = @("none")
        }
    }
} -Force
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$manifestJson = ($manifest | ConvertTo-Json -Depth 10).Replace('\u003c', '<').Replace('\u003e', '>')
[System.IO.File]::WriteAllText($manifestPath, $manifestJson, $utf8NoBom)
$firefoxZip = Join-Path $outDir "lajufile-extension-firefox.zip"
Write-ExtensionZip $firefoxStage $firefoxZip

Remove-Item -Recurse -Force $stageRoot
Get-Item $chromiumZip, $firefoxZip | Select-Object FullName, Length
