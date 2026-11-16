$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $pkg = (Get-Content -Raw "$root\Cargo.toml" | Select-String 'version = "([^"]+)"').Matches[0].Groups[1].Value
    $outDir = "$root\dist"
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    $outExe = "$outDir\LajuFile-v$pkg-Windows-x64-Setup.exe"

    cargo build --release -p lajufile-app --bin lajufile --bin lajufile-setup
    if ($LASTEXITCODE -ne 0) { throw "Release build failed" }

    $proc = Start-Process -FilePath "$root\target\release\lajufile-setup.exe" `
        -ArgumentList @(
            "--pack",
            "--stub", "$root\target\release\lajufile-setup.exe",
            "--app", "$root\target\release\lajufile.exe",
            "--ext", "$root\extension",
            "--license", "$root\LICENSE",
            "--readme", "$root\README.md",
            "--out", $outExe
        ) `
        -Wait -PassThru -NoNewWindow
    if ($proc.ExitCode -ne 0) { throw "Installer pack failed with exit code $($proc.ExitCode)" }

    $sizeMb = [Math]::Round((Get-Item $outExe).Length / 1MB, 2)
    Write-Host "Built Windows Installer: $outExe ($sizeMb MB)"
} finally {
    Pop-Location
}
