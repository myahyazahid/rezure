<#
.SYNOPSIS
Stages Nginx + PHP + the CA bundle into src-tauri/bundled-bin/ so `tauri build` can
embed them into the installer via tauri.conf.json's `bundle.resources` — see
services/binaries.rs's `seed_bundled()` and services/ca_bundle.rs's `seed_bundled()`,
which copy from there into the real install root on first launch.

Downloads, checksum-verifies, and extracts each package the same way
services::binaries::install_archive does, just headless (no running Tauri app to
emit progress to).

The two packages here mirror entries in services::binaries::MANIFEST
(src-tauri/src/services/binaries.rs) — keep the URL/sha256/exe path in sync if
those pins ever change. MariaDB and every other PHP version are intentionally
NOT staged here; they stay on-demand-only, downloaded from inside the app.

The CA bundle (curl.se's extract of Mozilla's root certificates) is pinned to a
*dated* URL, which never changes, rather than cacert.pem, which does. To refresh it
for a release, pick the newest dated file from https://curl.se/docs/caextract.html
and update $caBundle's Url + Sha256 together.

Idempotent: safe to re-run, skips anything already staged. Not part of the normal
dev loop (`npm run tauri dev` doesn't call this) — run manually, or from CI, before
a release build:

    npm run stage:binaries
#>

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue' # Invoke-WebRequest's default progress UI is slow in Windows PowerShell 5.1

$repoRoot = Split-Path -Parent $PSScriptRoot
$bundledBin = Join-Path $repoRoot 'src-tauri\bundled-bin'

$packages = @(
    @{
        Family          = 'nginx'
        Version         = '1.25.3'
        Url             = 'https://nginx.org/download/nginx-1.25.3.zip'
        Sha256          = '58df6e5865a922aaa477ac89b79c13739347a37ccc4b3de58de91f1487710cc4'
        ExeRelativePath = 'nginx-1.25.3\nginx.exe'
    },
    @{
        Family          = 'php'
        Version         = '8.3.33'
        Url             = 'https://downloads.php.net/~windows/releases/php-8.3.33-nts-Win32-vs16-x64.zip'
        Sha256          = '534399107056313246f424adbbb7937337e40fbbf6aa7bc26287ba9cfd2e4a2a'
        ExeRelativePath = 'php.exe'
    }
)

foreach ($pkg in $packages) {
    $label = "$($pkg.Family) $($pkg.Version)"
    $destDir = Join-Path $bundledBin "$($pkg.Family)\$($pkg.Version)"
    $exePath = Join-Path $destDir $pkg.ExeRelativePath

    if (Test-Path $exePath) {
        Write-Host "[$label] already staged, skipping."
        continue
    }

    Write-Host "[$label] downloading..."
    $zipPath = Join-Path ([System.IO.Path]::GetTempPath()) "rezure-stage-$($pkg.Family)-$($pkg.Version).zip"
    Invoke-WebRequest -Uri $pkg.Url -OutFile $zipPath -UseBasicParsing

    Write-Host "[$label] verifying checksum..."
    $actualHash = (Get-FileHash -Path $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $pkg.Sha256) {
        Remove-Item $zipPath -Force -ErrorAction SilentlyContinue
        throw "[$label] checksum mismatch: expected $($pkg.Sha256), got $actualHash"
    }

    Write-Host "[$label] extracting..."
    New-Item -ItemType Directory -Force -Path $destDir | Out-Null
    Expand-Archive -Path $zipPath -DestinationPath $destDir -Force
    Remove-Item $zipPath -Force

    if (-not (Test-Path $exePath)) {
        throw "[$label] expected $($pkg.ExeRelativePath) after extracting, but it was not found at $exePath"
    }

    Write-Host "[$label] staged at $destDir"
}

# PHP's Windows zip ships no CA store, so without this every HTTPS call out of PHP
# fails with cURL error 60 until the user fetches a bundle from inside the app.
$caBundle = @{
    Url    = 'https://curl.se/ca/cacert-2026-08-13.pem'
    Sha256 = 'f66dff1bdf8f96060b8177976f8b7d9254bc89bc4db933d769f7384d28480bc9'
}
$caDest = Join-Path $bundledBin 'ca\cacert.pem'

$caStaged = (Test-Path $caDest) -and
    ((Get-FileHash -Path $caDest -Algorithm SHA256).Hash.ToLowerInvariant() -eq $caBundle.Sha256)
if ($caStaged) {
    Write-Host "[ca bundle] already staged, skipping."
} else {
    Write-Host "[ca bundle] downloading..."
    $caTmp = Join-Path ([System.IO.Path]::GetTempPath()) 'rezure-stage-cacert.pem'
    Invoke-WebRequest -Uri $caBundle.Url -OutFile $caTmp -UseBasicParsing

    Write-Host "[ca bundle] verifying checksum..."
    $actualHash = (Get-FileHash -Path $caTmp -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $caBundle.Sha256) {
        Remove-Item $caTmp -Force -ErrorAction SilentlyContinue
        throw "[ca bundle] checksum mismatch: expected $($caBundle.Sha256), got $actualHash"
    }

    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $caDest) | Out-Null
    Move-Item -Path $caTmp -Destination $caDest -Force
    Write-Host "[ca bundle] staged at $caDest"
}

Write-Host "Done. Bundled binaries are in $bundledBin"
