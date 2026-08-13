[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$env:DATABASE_URL = 'postgresql://koserver:koserver123@localhost:5432/ko_server'
$utf8 = New-Object System.Text.UTF8Encoding($false)
[Console]::InputEncoding = $utf8
[Console]::OutputEncoding = $utf8
$OutputEncoding = $utf8

Push-Location $projectRoot
try {
    Write-Host 'Letter Admin hazırlanıyor...'
    & cargo build -p ko-letter-admin
    if ($LASTEXITCODE -ne 0) { throw 'ko-letter-admin derlenemedi.' }

    $backend = Join-Path $projectRoot 'target\debug\ko-letter-admin.exe'
    # Windows PowerShell 5 treats BOM-less UTF-8 .ps1 files as ANSI when they
    # are invoked directly. Reading explicitly as UTF-8 preserves Turkish UI.
    $uiPath = Join-Path $PSScriptRoot 'letter-admin.ps1'
    $uiSource = [System.IO.File]::ReadAllText($uiPath, $utf8)
    $uiScript = [ScriptBlock]::Create($uiSource)
    & $uiScript -Backend $backend
}
finally {
    Pop-Location
}
