param(
    [string]$PfxPath = $env:WINDOWS_PFX_PATH,
    [string]$PfxPassword = $env:WINDOWS_PFX_PASSWORD,
    [string]$TimestampUrl = $(if ($env:WINDOWS_TIMESTAMP_URL) { $env:WINDOWS_TIMESTAMP_URL } else { "http://timestamp.digicert.com" })
)

$ErrorActionPreference = "Stop"

function Find-SignTool {
    $cmd = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $kitsRoot = "C:\Program Files (x86)\Windows Kits\10\bin"
    if (-not (Test-Path $kitsRoot)) { return $null }
    return Get-ChildItem -Path $kitsRoot -Filter signtool.exe -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '\\x64\\signtool\.exe$' } |
        Sort-Object FullName -Descending |
        Select-Object -First 1 -ExpandProperty FullName
}

function Invoke-AuthenticodeSign([string]$TargetPath, [string]$SignTool, [string]$CertFile, [string]$Password, [string]$TsUrl) {
    $signArgs = @("sign", "/fd", "SHA256", "/tr", $TsUrl, "/td", "SHA256", "/f", $CertFile)
    if ($Password) {
        $signArgs += @("/p", $Password)
    }
    $signArgs += $TargetPath
    & $SignTool @signArgs
    if ($LASTEXITCODE -ne 0) { throw "signtool sign failed for $TargetPath" }
    & $SignTool verify /pa $TargetPath
    if ($LASTEXITCODE -ne 0) { throw "signtool verify failed for $TargetPath" }
}

$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
$tempPfx = $null
try {
    $pkg = (Get-Content -Raw "$root\Cargo.toml" | Select-String 'version = "([^"]+)"').Matches[0].Groups[1].Value
    $outDir = "$root\dist"
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    $outExe = "$outDir\LajuFile-v$pkg-Windows-x64-Setup.exe"

    cargo build --release -p lajufile-app --bin lajufile --bin lajufile-setup
    if ($LASTEXITCODE -ne 0) { throw "Release build failed" }

    if (-not $PfxPath -and $env:WINDOWS_PFX_BASE64) {
        $tempPfx = Join-Path ([System.IO.Path]::GetTempPath()) "lajufile-sign-$([guid]::NewGuid().ToString('N')).pfx"
        [System.IO.File]::WriteAllBytes($tempPfx, [Convert]::FromBase64String($env:WINDOWS_PFX_BASE64))
        $PfxPath = $tempPfx
    }
    $signTool = if ($PfxPath) { Find-SignTool } else { $null }
    if ($PfxPath -and -not $signTool) {
        throw "Signing requested via PFX, but signtool.exe was not found in PATH or Windows Kits."
    }

    # Sign the main app binary before packing; keep lajufile-setup.exe stub unsigned so extracted uninstall.exe stays valid.
    if ($signTool) {
        Invoke-AuthenticodeSign "$root\target\release\lajufile.exe" $signTool $PfxPath $PfxPassword $TimestampUrl
    }

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

    if ($signTool) {
        Invoke-AuthenticodeSign $outExe $signTool $PfxPath $PfxPassword $TimestampUrl
    }

    $hash = (Get-FileHash -Algorithm SHA256 -Path $outExe).Hash.ToLowerInvariant()
    $name = [System.IO.Path]::GetFileName($outExe)
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText("$outDir\SHA256SUMS.txt", "$hash  $name`n", $utf8NoBom)

    $sizeMb = [Math]::Round((Get-Item $outExe).Length / 1MB, 2)
    Write-Host "Built Windows Installer: $outExe ($sizeMb MB, sha256=$hash)"
} finally {
    if ($tempPfx -and (Test-Path $tempPfx)) {
        Remove-Item -Force $tempPfx -ErrorAction SilentlyContinue
    }
    Pop-Location
}
