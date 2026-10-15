[CmdletBinding()]
param([string]$Executable = (Join-Path $PSScriptRoot '..\target\release\process-garden.exe'))
$ErrorActionPreference = 'Stop'
$source = (Resolve-Path -LiteralPath $Executable).Path
$installDir = Join-Path $env:LOCALAPPDATA 'Programs\Process Garden'
$destination = Join-Path $installDir 'process-garden.exe'
if (Test-Path -LiteralPath $destination) {
    $running = Get-Process -Name process-garden -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $destination }
    if ($running) { throw 'Close the installed Process Garden before updating.' }
}
New-Item -ItemType Directory -Force -Path $installDir | Out-Null
Copy-Item -LiteralPath $source -Destination $destination -Force
$doc = Join-Path $PSScriptRoot '..\WINDOWS.md'
if (Test-Path -LiteralPath $doc) { Copy-Item -LiteralPath $doc -Destination (Join-Path $installDir 'WINDOWS.md') -Force }
$shell = New-Object -ComObject WScript.Shell
$shortcutDirectories = @([Environment]::GetFolderPath('Desktop'), [Environment]::GetFolderPath('Programs'))
foreach ($directory in $shortcutDirectories) {
    $shortcutPath = Join-Path $directory 'Process Garden.lnk'
    if (Test-Path -LiteralPath $shortcutPath) {
        $existing = $shell.CreateShortcut($shortcutPath)
        if ($existing.TargetPath -ne $destination) { throw "An unrelated shortcut already exists: $shortcutPath" }
    }
    $shortcut = $shell.CreateShortcut($shortcutPath)
    $shortcut.TargetPath = $destination
    $shortcut.WorkingDirectory = $installDir
    $shortcut.Description = 'Local Windows process and memory monitor'
    $shortcut.Save()
}
$sourceHash = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash
$installedHash = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash
if ($sourceHash -ne $installedHash) { throw 'Installed executable hash mismatch' }
[pscustomobject]@{ InstalledPath=$destination; SHA256=$installedHash; StartupRegistered=$false } | ConvertTo-Json
