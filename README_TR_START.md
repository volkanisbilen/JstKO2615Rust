# KORustGameServer Lokal Başlangıç

Bu dosyaları repo köküne kopyala.

## 1) PostgreSQL'i Docker ile başlat

```powershell
docker compose -f docker-compose.local.yml up -d
```

## 2) Git LFS SQL dosyalarını kontrol et

```powershell
powershell -ExecutionPolicy Bypass -File tools\check-lfs-pointers.ps1
```

Eğer hata verirse:

```powershell
git lfs install
git lfs pull
powershell -ExecutionPolicy Bypass -File tools\check-lfs-pointers.ps1
```

## 3) Rust build kontrolü

```powershell
cargo check -p ko-server
```

## 4) Server başlat

```powershell
powershell -ExecutionPolicy Bypass -File tools\run-local.ps1
```

Açılması gereken portlar:

- Login: 15100-15109
- Game: 15001
