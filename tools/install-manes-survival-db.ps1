$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
$sqlFile = Join-Path $PSScriptRoot "sql\manes-survival-postgresql.sql"

if (-not (Test-Path $sqlFile)) {
    throw "SQL installer not found: $sqlFile"
}

$container = "korust-postgres"
$running = docker ps --filter "name=^/$container$" --format "{{.Names}}"
if ($running -ne $container) {
    Write-Host "Starting local PostgreSQL container..." -ForegroundColor Cyan
    docker compose -f (Join-Path $repo "docker-compose.local.yml") up -d postgres
}

Write-Host "Installing Manes Survival database records..." -ForegroundColor Cyan
Get-Content -Raw -Encoding UTF8 $sqlFile |
    docker exec -i $container psql -v ON_ERROR_STOP=1 -U koserver -d ko_server

if ($LASTEXITCODE -ne 0) {
    throw "Manes Survival database installation failed (psql exit code $LASTEXITCODE)."
}

Write-Host "Manes Survival items, monsters and spawn configuration installed." -ForegroundColor Green
