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
$env:RUST_LOG = "info"

Write-Host "Starting ko-server with local PostgreSQL..." -ForegroundColor Cyan
cargo run -p ko-server
