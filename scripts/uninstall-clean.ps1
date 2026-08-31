<#
.SYNOPSIS
    Removes everything Rezure leaves behind outside its own install directory.

.DESCRIPTION
    Uninstalling the MSI removes the app, and nothing else. Rezure also writes
    to four places the installer never tracked, because they belong to the user
    rather than to the package:

      * the user PATH entry pointing at the PHP junction
      * a managed block in the Windows hosts file
      * %LOCALAPPDATA%\Rezure  - runtimes, nginx runtime state, its own MariaDB
                                 data directory, and the SQLite database
      * %APPDATA%\Rezure       - settings.json, profiles.json, links.json
      * %USERPROFILE%\rezure   - drop-in binaries, SQL dumps, the www root

    This script clears those. It never touches project source code, and never
    touches a Laragon or XAMPP data directory that Rezure merely adopted.

.PARAMETER Execute
    Actually delete. Without it the script only reports what it would do.

.PARAMETER KeepDumps
    Preserve %USERPROFILE%\rezure\dumps - exported .sql files are your data,
    not Rezure's.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/uninstall-clean.ps1
    Dry run: prints the plan, changes nothing.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/uninstall-clean.ps1 -Execute -KeepDumps
#>
[CmdletBinding()]
param(
    [switch]$Execute,
    [switch]$KeepDumps
)

$ErrorActionPreference = 'Stop'

$LocalRoot   = Join-Path $env:LOCALAPPDATA 'Rezure'
$RoamingRoot = Join-Path $env:APPDATA     'Rezure'
$HomeRoot    = Join-Path $env:USERPROFILE 'rezure'
$DumpsDir    = Join-Path $HomeRoot 'dumps'
$LinkDir     = Join-Path $LocalRoot 'current\php'
$HostsFile   = Join-Path $env:WINDIR 'System32\drivers\etc\hosts'

$BeginMarker = '# --- Rezure managed entries (do not edit below) ---'
$EndMarker   = '# --- Rezure managed entries end ---'

function Write-Step { param($Text) Write-Host "`n$Text" -ForegroundColor Cyan }
function Write-Act  { param($Text) Write-Host "  $Text" -ForegroundColor Gray }

if (-not $Execute) {
    Write-Host "DRY RUN - nothing will be changed. Re-run with -Execute to apply." -ForegroundColor Yellow
}

# --- 1. Stop anything still running -----------------------------------------
# A running php-cgi or mysqld holds an open handle inside the folders below,
# and the delete would fail halfway through with a partially removed tree.
Write-Step '1. Stopping Rezure processes'
$running = Get-Process -Name 'rezureapp', 'nginx', 'php-cgi', 'mysqld', 'mariadbd' -ErrorAction SilentlyContinue
if (-not $running) {
    Write-Act 'nothing running'
} else {
    foreach ($proc in $running) {
        Write-Act "stop $($proc.Name) (pid $($proc.Id))"
        if ($Execute) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
    }
}

# --- 2. Remove the PATH entry -----------------------------------------------
# Read raw and write back with the original value kind. A PATH holding
# %SystemRoot% is stored as REG_EXPAND_SZ; reading it expanded and writing it
# back as a plain string bakes today's values in permanently, which is the
# classic way PATH gets quietly mangled.
Write-Step '2. Removing the PATH entry'
$key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
try {
    $rawPath = $key.GetValue(
        'Path', '',
        [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $kind = $key.GetValueKind('Path')

    $remaining = @(
        $rawPath -split ';' | Where-Object {
            $_.Trim().TrimEnd('\') -and $_.Trim().TrimEnd('\') -ne $LinkDir.TrimEnd('\')
        }
    )
    $rebuilt = $remaining -join ';'

    if ($rebuilt -eq $rawPath) {
        Write-Act 'no Rezure entry in PATH'
    } else {
        Write-Act "remove: $LinkDir"
        if ($Execute) {
            $key.SetValue('Path', $rebuilt, $kind)
            # Without the broadcast, Explorer and every shell keep the old PATH
            # until the next sign-out.
            if (-not ('Win32.NativeMethods' -as [type])) {
                Add-Type -Namespace Win32 -Name NativeMethods -MemberDefinition @'
[DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)]
public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, UIntPtr wParam,
    string lParam, uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);
'@
            }
            $result = [UIntPtr]::Zero
            # HWND_BROADCAST, WM_SETTINGCHANGE, SMTO_ABORTIFHUNG
            [void][Win32.NativeMethods]::SendMessageTimeout(
                [IntPtr]0xFFFF, 0x1A, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$result)
            Write-Act 'PATH updated and broadcast'
        }
    }
} finally {
    if ($key) { $key.Close() }
}

# --- 3. Clean the hosts file ------------------------------------------------
Write-Step '3. Cleaning the hosts file'
$hostsText = Get-Content $HostsFile -Raw -ErrorAction SilentlyContinue
if (-not $hostsText -or $hostsText -notmatch [regex]::Escape($BeginMarker)) {
    Write-Act 'no Rezure block'
} else {
    $isAdmin = ([Security.Principal.WindowsPrincipal] `
        [Security.Principal.WindowsIdentity]::GetCurrent()
    ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

    $lines  = Get-Content $HostsFile
    $start  = [Array]::FindIndex([string[]]$lines, [Predicate[string]] { $args[0].Trim() -eq $BeginMarker })
    $end    = [Array]::FindIndex([string[]]$lines, [Predicate[string]] { $args[0].Trim() -eq $EndMarker })

    if ($start -lt 0 -or $end -lt $start) {
        Write-Act 'markers are malformed - edit the hosts file by hand'
    } else {
        ($lines[($start + 1)..($end - 1)] | Where-Object { $_.Trim() }) |
            ForEach-Object { Write-Act "remove: $($_.Trim())" }

        if ($Execute) {
            if (-not $isAdmin) {
                Write-Host "  SKIPPED - the hosts file needs an elevated shell." -ForegroundColor Yellow
                Write-Host "  Re-run this script as Administrator to remove the block." -ForegroundColor Yellow
            } else {
                # Trailing blank lines the block left behind go too, so running
                # this twice doesn't slowly grow the file.
                $kept = @()
                if ($start -gt 0)              { $kept += $lines[0..($start - 1)] }
                if ($end -lt $lines.Count - 1) { $kept += $lines[($end + 1)..($lines.Count - 1)] }
                while ($kept.Count -gt 0 -and -not $kept[-1].Trim()) {
                    $kept = $kept[0..($kept.Count - 2)]
                }
                Set-Content -Path $HostsFile -Value $kept -Encoding ASCII
                Write-Act 'hosts block removed'
            }
        }
    }
}

# --- 4. Delete the junctions before their parents ---------------------------
# Remove-Item -Recurse on a tree containing a junction can follow it and delete
# the *target* instead of the link. Directory.Delete removes the reparse point
# itself, so the PHP install it points at is left alone.
Write-Step '4. Removing directory junctions'
$junctions = @()
foreach ($root in @($LocalRoot, $HomeRoot)) {
    if (Test-Path $root) {
        $junctions += Get-ChildItem $root -Recurse -Directory -Force -ErrorAction SilentlyContinue |
            Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }
    }
}
if (-not $junctions) {
    Write-Act 'none found'
} else {
    foreach ($j in $junctions) {
        Write-Act "unlink: $($j.FullName)  ->  $($j.Target)"
        if ($Execute) { [System.IO.Directory]::Delete($j.FullName, $false) }
    }
}

# --- 5. Delete the data directories -----------------------------------------
Write-Step '5. Removing data directories'
$targets = @($LocalRoot, $RoamingRoot, $HomeRoot)

if ($KeepDumps -and (Test-Path $DumpsDir)) {
    $stash = Join-Path $env:USERPROFILE "rezure-dumps-kept-$(Get-Date -Format yyyyMMdd-HHmmss)"
    Write-Act "keep dumps -> $stash"
    if ($Execute) { Move-Item $DumpsDir $stash }
}

foreach ($dir in $targets) {
    if (-not (Test-Path $dir)) {
        Write-Act "already gone: $dir"
        continue
    }
    $size = (Get-ChildItem $dir -Recurse -File -Force -ErrorAction SilentlyContinue |
        Measure-Object Length -Sum).Sum
    Write-Act ("delete: {0}  ({1:N1} MB)" -f $dir, ($size / 1MB))
    if ($Execute) { Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue }
}

Write-Host ''
if ($Execute) {
    Write-Host 'Done. Open a new terminal for the PATH change to take effect.' -ForegroundColor Green
} else {
    Write-Host 'Dry run complete. Re-run with -Execute to apply.' -ForegroundColor Yellow
}
