$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

$files = Get-ChildItem -Path "migrations" -Filter "*.sql" -File -Recurse | Where-Object {
    Select-String -Path $_.FullName -Pattern "https://git-lfs.github.com/spec/v1" -Quiet
}

if ($files.Count -gt 0) {
    Write-Host "LFS pointer SQL files found. Run: git lfs pull" -ForegroundColor Red
    $files | ForEach-Object { Write-Host " - $($_.FullName)" }
    exit 1
}

Write-Host "OK: migration SQL files are real SQL, no LFS pointer detected." -ForegroundColor Green
