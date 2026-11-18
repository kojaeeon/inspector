param([switch]$Remove)
$ErrorActionPreference = 'Stop'
$exe = Join-Path $PSScriptRoot 'inspector.exe'
$startup = [Environment]::GetFolderPath('Startup')
$linkPath = Join-Path $startup 'Office Inspector.lnk'
if ($Remove) {
    Remove-Item $linkPath -ErrorAction SilentlyContinue
    Write-Host 'Startup shortcut removed.'
    exit
}
if (!(Test-Path $exe)) { throw "Missing inspector.exe: $exe" }
$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut($linkPath)
$link.TargetPath = $exe
$link.Arguments = 'watch --continuous --out office-auth.jsonl'
$link.WorkingDirectory = $PSScriptRoot
$link.WindowStyle = 7
$link.Save()
Write-Host "Startup shortcut installed: $linkPath"
Write-Host 'Note: existing office-auth.jsonl blocks a new capture; archive/remove it before next login.'
