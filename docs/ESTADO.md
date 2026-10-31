# Estado de Desván

## Hecho
- `crates/core`: lector directo de la MFT de NTFS (sector de arranque, data runs, registros FILE con fixups, $ATTRIBUTE_LIST, flujos ADS, dispersos y comprimidos), lectura paralela por bloques con el $BITMAP de la MFT, árbol compacto (CSR) con huérfanos, ciclos y enlaces duros.
- Pruebas con imágenes NTFS reales creadas en WSL (`scripts/make-ntfs-images.sh`): coincide con ntfs-3g en rutas, tamaños, enlaces y tamaño en disco.
- `crates/cli`: `desvan scan C:` / `desvan scan --image x.img`.

## En curso
- Treemap (layout en Rust + WebGL2), escaneo sin administrador, duplicados y limpieza.
- App Tauri 2 (`app/`).
