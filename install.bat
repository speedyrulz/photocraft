@echo off
rem Installs RulZ Photocraft from the newest release of speedyrulz/rulz-photocraft and puts a
rem shortcut on the desktop. update.bat calls this file with --update. No admin rights, and no
rem developer tools: it downloads the built app. The PowerShell below does the work.
rem
rem The PowerShell line ends with its own exit on the same line. An update replaces this file
rem while it runs, and cmd reads a batch file line by line, so it must read nothing afterwards.
setlocal EnableDelayedExpansion
set "RULZ_SCRIPT=%~f0"
set "RULZ_ARGS=%*"
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "$s = Get-Content -LiteralPath $env:RULZ_SCRIPT -Raw; Invoke-Expression $s.Substring($s.IndexOf('#POWERSHELL' + '-STARTS-HERE'))" & exit /b !errorlevel!
#POWERSHELL-STARTS-HERE
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$Repo = 'speedyrulz/rulz-photocraft'     # Updates come from this repository and no other.
$Name = 'RulZ Photocraft'
$Tokens = -split $env:RULZ_ARGS
$Update = $Tokens -contains '--update'
$WaitPid = 0
$at = [array]::IndexOf($Tokens, '--wait-pid')
if ($at -ge 0) { [void][int]::TryParse($Tokens[$at + 1], [ref]$WaitPid) }
$FromApp = $WaitPid -gt 0

function Finish([int] $Code) {
    # The window closes when this ends, so a double-clicked script waits to be read. The app's
    # own update only waits if something went wrong.
    if (-not $FromApp -or $Code -ne 0) { [void](Read-Host 'Press Enter to close') }
    exit $Code
}

function Fail([string] $Message) {
    Write-Host ''
    Write-Host "Error: $Message" -ForegroundColor Red
    Finish 1
}

# update.bat installed beside the app updates that copy; otherwise the standard folder.
$Here = Split-Path -Parent $env:RULZ_SCRIPT
if (Test-Path -LiteralPath (Join-Path $Here 'version.txt')) { $Folder = $Here }
else { $Folder = Join-Path $env:LOCALAPPDATA "Programs\$Name" }
$Exe = Join-Path $Folder 'photocraft.exe'
$VersionFile = Join-Path $Folder 'version.txt'

if (-not [Environment]::Is64BitOperatingSystem) { Fail "$Name needs 64-bit Windows." }
if ($Update -and -not (Test-Path -LiteralPath $Exe)) { Fail "$Name isn't installed in $Folder. Run install.bat first." }

Write-Host "Checking $Repo for the newest release..."
try {
    $Release = Invoke-RestMethod -UseBasicParsing -Headers @{ 'User-Agent' = 'Photocraft-installer' } `
        -Uri "https://api.github.com/repos/$Repo/releases/latest"
} catch {
    Fail "Could not read the newest release from GitHub: $($_.Exception.Message)"
}
$Version = $Release.tag_name -replace '^[vV]', ''
try { $Latest = [version]($Version -replace '[-+].*$', '') } catch { Fail "GitHub reports an unreadable version: $($Release.tag_name)" }

if ($Update -and (Test-Path -LiteralPath $VersionFile)) {
    $Have = (Get-Content -LiteralPath $VersionFile -Raw).Trim()
    try {
        if ([version]($Have -replace '[-+].*$', '') -ge $Latest) {
            Write-Host "$Name $Have is up to date."
            Finish 0
        }
    } catch { Write-Host "Reinstalling: the installed version '$Have' can't be read." }
}

$ZipName = "photocraft-$Version-windows-x64-portable.zip"
$Zip = $Release.assets | Where-Object { $_.name -eq $ZipName } | Select-Object -First 1
$Sums = $Release.assets | Where-Object { $_.name -eq 'SHA256SUMS.txt' } | Select-Object -First 1
if (-not $Zip -or -not $Sums) { Fail "Release $($Release.tag_name) has no $ZipName with a SHA256SUMS.txt." }

$Work = Join-Path ([IO.Path]::GetTempPath()) ("rulz-photocraft-" + [guid]::NewGuid())
$Staged = Join-Path $Work 'app'
$Done = @()
try {
    New-Item -ItemType Directory -Force -Path $Work | Out-Null

    Write-Host "Downloading $Name $Version..."
    $ZipPath = Join-Path $Work $ZipName
    $SumsPath = Join-Path $Work 'SHA256SUMS.txt'
    Invoke-WebRequest -UseBasicParsing -Uri $Sums.browser_download_url -OutFile $SumsPath
    Invoke-WebRequest -UseBasicParsing -Uri $Zip.browser_download_url -OutFile $ZipPath

    $Line = Get-Content -LiteralPath $SumsPath | Where-Object { ($_ -split '\s+\*?')[1] -eq $ZipName } | Select-Object -First 1
    if (-not $Line) { throw "SHA256SUMS.txt doesn't list $ZipName." }
    $Want = ($Line -split '\s+')[0]
    $Got = (Get-FileHash -LiteralPath $ZipPath -Algorithm SHA256).Hash
    if ($Got -ne $Want) { throw "The download is damaged: its checksum doesn't match SHA256SUMS.txt." }

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [IO.Compression.ZipFile]::ExtractToDirectory($ZipPath, $Staged)
    if (-not (Test-Path -LiteralPath (Join-Path $Staged 'photocraft.exe'))) { throw "$ZipName has no photocraft.exe." }
    # Settings stay in the normal per-user folders, which outlive the app's own folder.
    Remove-Item -LiteralPath (Join-Path $Staged 'portable.txt') -Force -ErrorAction SilentlyContinue
    Set-Content -LiteralPath (Join-Path $Staged 'version.txt') -Value $Version -Encoding ASCII

    if ($WaitPid -gt 0) {
        Write-Host "Waiting for $Name to close..."
        Wait-Process -Id $WaitPid -ErrorAction SilentlyContinue
    }
    while (Get-Process -Name photocraft -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $Exe }) {
        Write-Host "Close $Name to continue..."
        Start-Sleep -Seconds 2
    }

    # File by file, keeping each old file until all the new ones are in, so a failure part-way
    # (a full disk, a locked file) puts the working copy back.
    Write-Host "Installing to $Folder..."
    New-Item -ItemType Directory -Force -Path $Folder | Out-Null
    Get-ChildItem -LiteralPath $Staged -File | ForEach-Object {
        $Target = Join-Path $Folder $_.Name
        $Old = "$Target.old"
        Remove-Item -LiteralPath $Old -Force -ErrorAction SilentlyContinue
        if (Test-Path -LiteralPath $Target) { Move-Item -LiteralPath $Target -Destination $Old }
        $Done += $Target
        Copy-Item -LiteralPath $_.FullName -Destination $Target
    }
} catch {
    foreach ($Target in $Done) {
        Remove-Item -LiteralPath $Target -Force -ErrorAction SilentlyContinue
        if (Test-Path -LiteralPath "$Target.old") { Move-Item -LiteralPath "$Target.old" -Destination $Target -Force }
    }
    Remove-Item -LiteralPath $Work -Recurse -Force -ErrorAction SilentlyContinue
    Fail $_.Exception.Message
}
foreach ($Target in $Done) { Remove-Item -LiteralPath "$Target.old" -Force -ErrorAction SilentlyContinue }
Remove-Item -LiteralPath $Work -Recurse -Force -ErrorAction SilentlyContinue

if (-not $Update) {
    try {
        $Link = Join-Path ([Environment]::GetFolderPath('Desktop')) "$Name.lnk"
        $Shell = New-Object -ComObject WScript.Shell
        $Shortcut = $Shell.CreateShortcut($Link)
        $Shortcut.TargetPath = $Exe
        $Shortcut.WorkingDirectory = $Folder
        $Shortcut.IconLocation = "$Exe,0"
        $Shortcut.Description = $Name
        $Shortcut.Save()
        Write-Host "Added a shortcut to your desktop."
    } catch {
        Write-Host "$Name is installed, but the desktop shortcut failed: $($_.Exception.Message)" -ForegroundColor Yellow
    }
}

Write-Host ''
Write-Host "$Name $Version is installed." -ForegroundColor Green
if ($FromApp) { Start-Process -FilePath $Exe -WorkingDirectory $Folder }
Finish 0
