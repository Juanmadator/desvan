# Mide el escaneo por MFT de una unidad real. Necesita administrador: lanza la CLI elevada
# (aparece el aviso de UAC) y guarda el resultado en captures/bench-<unidad>.json.
# Uso: powershell -File scripts/bench-admin.ps1 [-Drive C] [-Repeat 3]
param([string]$Drive = "C", [int]$Repeat = 3)
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root "target\release\desvan.exe"
$out = Join-Path $root "captures\bench-$($Drive.ToLower()).json"
$log = Join-Path $root "captures\bench-$($Drive.ToLower()).log"
New-Item -ItemType Directory -Force (Join-Path $root "captures") | Out-Null
$cmd = "& '$exe' bench $($Drive): --repeat $Repeat 1> '$out' 2> '$log'"
$p = Start-Process powershell -Verb RunAs -WindowStyle Hidden -Wait -PassThru -ArgumentList "-NoProfile", "-Command", $cmd
"código de salida: $($p.ExitCode)"
Get-Content $log
