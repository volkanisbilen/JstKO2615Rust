$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

$env:DATABASE_URL = "postgresql://koserver:koserver123@localhost:5432/ko_server"
$env:BIND_IP = "0.0.0.0"
$env:BIND_ADDR = "0.0.0.0:15001"
$env:BASE_PORT = "15100"
$env:GAME_SERVER_IP = "127.0.0.1"
$env:GAME_SERVER_PORT = "15001"
$env:MAP_DIR = "./Map"
$env:RUST_LOG = "ko_game=debug,ko_server=debug,ko_protocol=debug,info"
$env:KO_VERSION_MODE = "99"

Write-Host "Starting ko-server with local PostgreSQL..." -ForegroundColor Cyan
$logDir = Join-Path $repo "logs"
New-Item -ItemType Directory -Path $logDir -Force | Out-Null

$timestamp = Get-Date -Format "yyyyMMdd_HHmmss"
$logFile = Join-Path $logDir "ko-server_$timestamp.log"

Write-Host "Log file: $logFile" -ForegroundColor Yellow

cmd /d /s /c "cargo run -p ko-server 2>&1" | Tee-Object -FilePath $logFile
