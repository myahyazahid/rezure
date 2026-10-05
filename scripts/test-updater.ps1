<#
.SYNOPSIS
  Tests Rezure's auto-update end to end on this machine, with a THROWAWAY
  signing key — the production key is never needed, and never touched.

.DESCRIPTION
  The updater trusts one public key, baked into the binary at build time
  (plugins.updater.pubkey). So an update can only be tested for real by an app
  that trusts a key we hold. This script:

    1. generates a dev key (and a second, "wrong" one);
    2. builds Rezure with that dev key as its trusted key and a local
       http://127.0.0.1 endpoint — through a config *override file*, so
       tauri.conf.json and the source are not changed;
    3. signs the installer it built, and writes three manifests:
         good       signed with the dev key            -> must be offered and accepted
         launch     a harmless .exe, signed correctly  -> must be accepted AND run, then the app exits
         wrong-key  signed with a different key        -> must be REFUSED
         tampered   right signature, one bit flipped   -> must be REFUSED
    4. runs the real binary against each (scripts/updater-probe.mjs) and
       reports what the user would see.

  "good" stops at "an update is available". Pressing Update would install a
  new Rezure on this machine (over an installed one, if there is one), so that
  last step is yours (see -Install). "launch" covers the part before it safely:
  the "installer" it offers is a copy of Windows' own hostname.exe, signed with
  the dev key, so the app downloads it, accepts the signature, runs it and quits
  — exactly what it does with a real installer — without installing anything.

  By default the installer offered is the one just built (so the manifest says a
  higher version than the installer actually is — detection, download and
  signature checks are all real; the version after installing is not). Pass
  -RealUpdate to build a genuine higher version as well, which is what proves
  the version changes.

.EXAMPLE
  .\scripts\test-updater.ps1                    # build once, run all three scenarios
  .\scripts\test-updater.ps1 -Run wrong-key     # one scenario, reusing the build
  .\scripts\test-updater.ps1 -Run serve         # serve "good" for a manual install test
  .\scripts\test-updater.ps1 -RealUpdate -Rebuild
#>
[CmdletBinding()]
param(
    [ValidateSet('all', 'prepare', 'good', 'launch', 'wrong-key', 'tampered', 'serve')]
    [string]$Run = 'all',
    [int]$Port = 8777,
    [string]$Work = (Join-Path $env:TEMP 'rezure-updater-test'),
    [switch]$Rebuild,
    [switch]$RealUpdate,
    [switch]$Install
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$Conf = Get-Content (Join-Path $Root 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json
$CurrentVersion = $Conf.version
$Password = 'rezure-updater-test'   # a throwaway key's password; protects nothing
$Utf8 = New-Object System.Text.UTF8Encoding $false

function Bump-Patch([string]$version) {
    $parts = $version.Split('.')
    "$($parts[0]).$($parts[1]).$([int]$parts[2] + 1)"
}
$OfferedVersion = Bump-Patch $CurrentVersion

# Run from the repo root so `npx tauri` finds the project's CLI.
Push-Location $Root
try {
    $Keys = Join-Path $Work 'keys'
    $App = Join-Path $Work 'app'
    $Payload = Join-Path $Work 'serve'
    New-Item -ItemType Directory -Force $Keys, $App, $Payload | Out-Null

    function Write-Json([string]$path, $value) {
        [IO.File]::WriteAllText($path, ($value | ConvertTo-Json -Depth 10), $Utf8)
    }

    # ---- 1. keys -------------------------------------------------------------
    function New-Key([string]$name) {
        $path = Join-Path $Keys $name
        if ($Rebuild -or -not (Test-Path $path)) {
            npx tauri signer generate --ci -p $Password -w $path -f | Out-Null
        }
        $path
    }
    $DevKey = New-Key 'dev.key'
    $WrongKey = New-Key 'wrong.key'
    $DevPub = (Get-Content "$DevKey.pub" -Raw).Trim()

    # ---- 2. build ------------------------------------------------------------
    function Build-App([string]$version) {
        # JSON merge patch over tauri.conf.json. `resources = $null` removes the
        # bundled PHP/nginx/CA payload: it has nothing to do with updating, and
        # without it the installer is a fraction of the size.
        $override = @{
            version = $version
            plugins = @{
                updater = @{
                    pubkey                            = $DevPub
                    endpoints                         = @("http://127.0.0.1:$Port/latest.json")
                    dangerousInsecureTransportProtocol = $true
                }
            }
            bundle  = @{ resources = $null; createUpdaterArtifacts = $true; targets = @('nsis') }
        }
        $file = Join-Path $Work "override-$version.json"
        Write-Json $file $override

        $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $DevKey -Raw)
        $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $Password
        Write-Host "==> building Rezure $version (dev key, endpoint http://127.0.0.1:$Port)" -ForegroundColor Cyan
        npx tauri build -c $file -b nsis
        if ($LASTEXITCODE -ne 0) { throw "tauri build failed ($LASTEXITCODE)" }

        $installer = Get-ChildItem (Join-Path $Root 'src-tauri\target\release\bundle\nsis') -Filter "*_${version}_*-setup.exe" |
            Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if (-not $installer) { throw "no installer for $version was produced" }
        $installer
    }

    $InstalledApp = Join-Path $App 'rezureapp.exe'
    $Offered = Get-ChildItem $Payload -Filter '*-setup.exe' -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -notmatch 'tampered' } | Select-Object -First 1

    if ($Rebuild -or -not (Test-Path $InstalledApp) -or -not $Offered) {
        # The "installed" app is the OLD version: it trusts the dev key and asks
        # the local endpoint. Keep its binary, because the next build overwrites it.
        $old = Build-App $CurrentVersion
        Copy-Item (Join-Path $Root 'src-tauri\target\release\rezureapp.exe') $InstalledApp -Force
        Copy-Item $old.FullName (Join-Path $App $old.Name) -Force

        $offeredInstaller = $old
        if ($RealUpdate) { $offeredInstaller = Build-App $OfferedVersion }
        Get-ChildItem $Payload | Remove-Item -Force
        Copy-Item $offeredInstaller.FullName $Payload -Force
        $Offered = Get-Item (Join-Path $Payload $offeredInstaller.Name)
    }

    # ---- 3. sign + manifests -------------------------------------------------
    # The build read the dev key from the environment; `signer sign -f` below
    # takes a key file, and the two would conflict.
    Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY, Env:\TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue

    # The build already signed $Offered with the dev key when the updater
    # artifacts were on; sign again explicitly so this works on a reused build.
    $Sig = "$($Offered.FullName).sig"
    npx tauri signer sign -f $DevKey -p $Password $Offered.FullName | Out-Null
    $GoodSignature = (Get-Content $Sig -Raw).Trim()

    # Same bytes, signed by a key the app does not trust.
    $wrongCopy = Join-Path $Work 'signed-by-wrong-key.exe'
    Copy-Item $Offered.FullName $wrongCopy -Force
    npx tauri signer sign -f $WrongKey -p $Password $wrongCopy | Out-Null
    $WrongSignature = (Get-Content "$wrongCopy.sig" -Raw).Trim()

    # The right signature on a file that is no longer the one signed.
    $tamperedName = 'tampered-setup.exe'
    $bytes = [IO.File]::ReadAllBytes($Offered.FullName)
    $bytes[[int]($bytes.Length / 2)] = $bytes[[int]($bytes.Length / 2)] -bxor 1
    [IO.File]::WriteAllBytes((Join-Path $Payload $tamperedName), $bytes)

    function Write-Manifest([string]$scenario, [string]$signature, [string]$fileName) {
        Write-Json (Join-Path $Payload "latest.$scenario.json") @{
            version   = $OfferedVersion
            notes     = "Local updater test ($scenario)"
            pub_date  = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
            platforms = @{ 'windows-x86_64' = @{ signature = $signature; url = "http://127.0.0.1:$Port/$fileName" } }
        }
    }
    # A stand-in installer that does nothing. Named without "-setup" so it is
    # never mistaken for the real one above.
    $launchName = 'launch-check.exe'
    Copy-Item (Join-Path $env:SystemRoot 'System32\hostname.exe') (Join-Path $Payload $launchName) -Force
    npx tauri signer sign -f $DevKey -p $Password (Join-Path $Payload $launchName) | Out-Null
    $LaunchSignature = (Get-Content (Join-Path $Payload "$launchName.sig") -Raw).Trim()

    Write-Manifest 'good'      $GoodSignature  $Offered.Name
    Write-Manifest 'launch'    $LaunchSignature $launchName
    Write-Manifest 'wrong-key' $WrongSignature $Offered.Name
    Write-Manifest 'tampered'  $GoodSignature  $tamperedName

    Write-Host "==> ready: app $InstalledApp, offering $OfferedVersion from $Payload" -ForegroundColor Green
    if ($Run -eq 'prepare') { return }

    # ---- 4. run --------------------------------------------------------------
    if ($Run -eq 'serve') {
        Write-Host @"

Manual install test:
  1. Install the OLD app:  $(Join-Path $App ($Offered.Name -replace "_${OfferedVersion}_", "_${CurrentVersion}_"))   (or run $InstalledApp directly)
  2. Leave this window open (it serves the update). Open Rezure -> Changelog.
  3. 'Rezure $OfferedVersion is available' -> press Update. It downloads, verifies, installs, relaunches.
Uninstall afterwards: Settings -> Apps -> Rezure.

"@
        node (Join-Path $PSScriptRoot 'updater-probe.mjs') --serve-only --dir $Payload --port $Port --scenario good
        return
    }

    $scenarios = if ($Run -eq 'all') { @('good', 'launch', 'wrong-key', 'tampered') } else { @($Run) }
    $failed = @()
    foreach ($scenario in $scenarios) {
        Write-Host "==> scenario: $scenario" -ForegroundColor Cyan
        $probe = @('--exe', $InstalledApp, '--dir', $Payload, '--port', $Port, '--scenario', $scenario, '--version', $OfferedVersion)
        if ($Install -and $scenario -eq 'good') { $probe += '--install' }
        node (Join-Path $PSScriptRoot 'updater-probe.mjs') @probe
        if ($LASTEXITCODE -ne 0) { $failed += $scenario }
    }
    if ($failed.Count -gt 0) {
        Write-Host "FAILED: $($failed -join ', ')" -ForegroundColor Red
        exit 1
    }
    Write-Host "All scenarios behaved as expected." -ForegroundColor Green
}
finally {
    Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY, Env:\TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
    Pop-Location
}
