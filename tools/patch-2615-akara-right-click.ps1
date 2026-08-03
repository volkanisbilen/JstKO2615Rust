param(
    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$ClientExe
)

$ErrorActionPreference = "Stop"

$clientPath = (Resolve-Path -LiteralPath $ClientExe).Path
$image = [System.IO.File]::ReadAllBytes($clientPath)

# KnightOnLine 2615 unpack, ImageBase 0x00400000.
# CGameProcMain right-click dispatch at VA 0x007FB366:
#   preserve the original sub_7EA360 result;
#   when that filter rejects the object, allow only proto 30001 (Akara);
#   all other rejected client objects keep the original path.
$dispatchOffset = 0x003FB366
$caveOffset = 0x000B00D9

$expectedDispatch = [byte[]](0x84,0xC0,0x0F,0x84,0xA0,0x00,0x00,0x00)
$patchedDispatch  = [byte[]](0xE9,0x6E,0x4D,0xCB,0xFF,0x90,0x90,0x90)
$caveCode = [byte[]](
    0x84,0xC0,
    0x0F,0x85,0x8D,0xB2,0x34,0x00,
    0x81,0xBF,0x8C,0x0B,0x00,0x00,0x31,0x75,0x00,0x00,
    0x0F,0x84,0x7D,0xB2,0x34,0x00,
    0xE9,0x18,0xB3,0x34,0x00
)

function Test-Bytes {
    param([byte[]]$Data, [int]$Offset, [byte[]]$Expected)
    if ($Offset -lt 0 -or $Offset + $Expected.Length -gt $Data.Length) {
        return $false
    }
    for ($i = 0; $i -lt $Expected.Length; $i++) {
        if ($Data[$Offset + $i] -ne $Expected[$i]) {
            return $false
        }
    }
    return $true
}

if (Test-Bytes $image $dispatchOffset $patchedDispatch) {
    Write-Host "Akara sağ tık düzeltmesi zaten uygulanmış: $clientPath" -ForegroundColor Yellow
    exit 0
}

if (-not (Test-Bytes $image $dispatchOffset $expectedDispatch)) {
    throw "Bu EXE doğrulanmış KnightOnLine 2615 yapısıyla eşleşmiyor (VA 0x007FB366 imzası farklı). Dosya değiştirilmedi."
}

for ($i = 0; $i -lt $caveCode.Length; $i++) {
    if ($image[$caveOffset + $i] -ne 0xCC) {
        throw "Doğrulanmış code-cave boş değil (VA 0x004B00D9). Dosya değiştirilmedi."
    }
}

$backupPath = "{0}.before-akara-{1}.bak" -f $clientPath, (Get-Date -Format "yyyyMMdd-HHmmss")
Copy-Item -LiteralPath $clientPath -Destination $backupPath

[Array]::Copy($patchedDispatch, 0, $image, $dispatchOffset, $patchedDispatch.Length)
[Array]::Copy($caveCode, 0, $image, $caveOffset, $caveCode.Length)

$tempPath = "$clientPath.akara.tmp"
try {
    [System.IO.File]::WriteAllBytes($tempPath, $image)
    $verify = [System.IO.File]::ReadAllBytes($tempPath)
    if (-not (Test-Bytes $verify $dispatchOffset $patchedDispatch) -or
        -not (Test-Bytes $verify $caveOffset $caveCode)) {
        throw "Yazılan geçici EXE doğrulanamadı."
    }
    Move-Item -LiteralPath $tempPath -Destination $clientPath -Force
}
catch {
    if (Test-Path -LiteralPath $tempPath) {
        Remove-Item -LiteralPath $tempPath -Force
    }
    throw
}

$hash = (Get-FileHash -LiteralPath $clientPath -Algorithm SHA256).Hash
Write-Host "Akara sağ tık düzeltmesi uygulandı." -ForegroundColor Green
Write-Host "Client: $clientPath" -ForegroundColor Cyan
Write-Host "Yedek:  $backupPath" -ForegroundColor Cyan
Write-Host "SHA256: $hash" -ForegroundColor Cyan
