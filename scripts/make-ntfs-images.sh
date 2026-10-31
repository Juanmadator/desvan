#!/bin/bash
# Crea imágenes NTFS de prueba con mkntfs + ntfs-3g (en WSL, como root) y un manifiesto
# JSON con lo que debería ver el escáner. Uso (desde Windows):
#   wsl -u root -- bash scripts/make-ntfs-images.sh
set -euo pipefail
cd "$(dirname "$0")/.."
OUT="$PWD/testdata"
WORK=/tmp/desvan-img
MNT=/mnt/desvan-img
mkdir -p "$OUT" "$WORK" "$MNT"

make_image() { # nombre tamaño_MB tamaño_cluster
  local name=$1 size=$2 cluster=$3
  local img="$WORK/$name.img"
  rm -f "$img"
  truncate -s "${size}M" "$img"
  mkntfs -F -Q -q -c "$cluster" -L "$name" "$img"
  ntfs-3g -o streams_interface=xattr,big_writes "$img" "$MNT"
}

finish_image() { # nombre
  local name=$1
  sync
  python3 - "$MNT" > "$WORK/$name.json" <<'PY'
import json, os, sys
root = sys.argv[1]
items = []
for dirpath, dirnames, filenames in os.walk(root):
    for n in dirnames + filenames:
        p = os.path.join(dirpath, n)
        st = os.lstat(p)
        rel = os.path.relpath(p, root).replace("/", "\\")
        items.append({"path": rel, "dir": os.path.isdir(p) and not os.path.islink(p), "size": st.st_size if not os.path.isdir(p) else 0,
                      "blocks": st.st_blocks * 512, "ino": st.st_ino, "nlink": st.st_nlink})
print(json.dumps({"items": items}, ensure_ascii=False, indent=0))
PY
  umount "$MNT"
  cp "$WORK/$name.img" "$WORK/$name.json" "$OUT/"
  echo "imagen $name: $(du -h "$OUT/$name.img" | cut -f1)"
}

# ---------- imagen 1: casos difíciles ----------
if [ "${ONLY:-}" != grande ]; then
make_image basico 96 4096
cd "$MNT"
mkdir -p "Documentos/Facturas 2026" "Fotos/Vacaciones ñandú" "Código/proyecto/src" "Juegos" "日本語フォルダ" "vacía"
printf '' > "Documentos/vacío.txt"
printf 'x' > "Documentos/un byte.txt"
head -c 700 /dev/urandom > "Documentos/pequeño residente.bin"
head -c 4096 /dev/urandom > "Documentos/Facturas 2026/factura-001.pdf"
head -c 1048576 /dev/urandom > "Fotos/Vacaciones ñandú/playa.jpg"
head -c 5242880 /dev/urandom > "Juegos/datos.pak"
head -c 123457 /dev/urandom > "Código/proyecto/src/main.rs"
head -c 3000 /dev/urandom > "日本語フォルダ/ファイル 🎮.txt"
# nombre largo (240 caracteres)
LONG=$(printf 'n%.0s' $(seq 1 236)).txt
head -c 100 /dev/urandom > "Documentos/$LONG"
# enlaces duros: 3 nombres para un archivo
head -c 300000 /dev/urandom > "Fotos/original.raw"
ln "Fotos/original.raw" "Documentos/enlace.raw"
ln "Fotos/original.raw" "Código/otro enlace.raw"
# 40 enlaces: desborda el registro y obliga a usar $ATTRIBUTE_LIST
head -c 200000 /dev/urandom > "Juegos/compartido.dat"
mkdir -p "Juegos/enlaces"
for i in $(seq 1 40); do ln "Juegos/compartido.dat" "Juegos/enlaces/copia-con-un-nombre-bastante-largo-para-llenar-el-registro-$i.dat"; done
# flujo alternativo (ADS)
head -c 9000 /dev/urandom > "Documentos/con flujo.txt"
setfattr -n user.Zone.Identifier -v "$(head -c 6000 /dev/urandom | base64 -w0)" "Documentos/con flujo.txt"
# disperso: 40 MB lógicos con un trozo de datos en medio
truncate -s 40M "Juegos/disperso.vhd"
dd if=/dev/urandom of="Juegos/disperso.vhd" bs=65536 count=2 seek=300 conv=notrunc status=none
# borrados (registros que dejan de estar en uso)
for i in $(seq 1 50); do head -c 5000 /dev/urandom > "vacía/tmp$i"; done
rm -f vacía/tmp*
# muchos archivos para que la MFT ocupe varios bloques de lectura
mkdir -p "Código/node_modules"
for d in $(seq 1 60); do
  mkdir -p "Código/node_modules/paquete-$d/lib"
  for f in $(seq 1 80); do printf 'module.exports = %d;\n' $f > "Código/node_modules/paquete-$d/lib/f$f.js"; done
done
# archivo fragmentado: dos archivos que crecen intercalados
for i in $(seq 1 120); do
  head -c 8192 /dev/urandom >> "Juegos/fragmentado-a.bin"
  head -c 8192 /dev/urandom >> "Juegos/fragmentado-b.bin"
  sync
done
cd /
finish_image basico
fi

# ---------- imagen 2: rendimiento (100 000 archivos) ----------
make_image grande 768 4096
cd "$MNT"
python3 - <<'PY'
import os, random
random.seed(7)
for d in range(400):
    base = f"carpeta-{d:03d}/sub-{d % 17}"
    os.makedirs(base, exist_ok=True)
    for f in range(250):
        size = random.choice([0, 10, 300, 900, 2000, 5000, 9000])
        with open(f"{base}/archivo-{f:03d}.dat", "wb") as fh:
            if size:
                fh.write(os.urandom(size))
PY
cd /
finish_image grande
echo "listo"
