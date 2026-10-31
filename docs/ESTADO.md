# Estado de Desván

Última actualización: 2026-11-01, madrugada.

## Hecho
- **Núcleo (`crates/core`)**: lector de la MFT (verificado contra ntfs-3g y en el C: real: 3,36 M de archivos en ~4 s), recorrido sin administrador (`walk.rs`), árbol compacto, treemap squarified con cojines (`layout.rs`), categorías, duplicados (`dupes.rs`), reglas de limpieza (`junk.rs`), serialización (`snapshot.rs`), ayudante elevado por tubería (`helper.rs`), Papelera y restauración (`win.rs`).
- **App (`app/`)**: Tauri 2 + Svelte 5 + renderizador WebGL2. Inicio, escaneo, explorador (árbol, mapa, detalles, tipos, mayores, limpieza, duplicados), borrado con confirmación.
- **Instalada** en el equipo del usuario (`%LOCALAPPDATA%\Desván`, versión del commit 221ad69, sin «Deshacer»).

## Sin terminar (lo último en curso)
- **Deshacer borrado**: backend (`undo_delete`) e interfaz (aviso con «Deshacer») escritos y compilando; `restore` probado contra la Papelera real. Falta pasar `scripts/e2e.mjs` (no conectaba por CDP al lanzar la app directamente con la app instalada abierta; con `cmd /c` sí: investigar) y reinstalar.
- La ventana se crea ahora en `setup` (`lib.rs`) para poder usar `DESVAN_WEBVIEW_DATA` en pruebas: comprobar que la versión instalada abre bien.

## Siguiente
1. Pasar el e2e, `npx tauri build`, reinstalar (`/S`) y abrir la app.
2. Reanálisis instantáneo: guardar instantánea (`snapshot.rs`) + diario USN.
3. README con capturas (`docs/img`) y release en GitHub con el instalador.

## Notas
- Pruebas visuales: `node scripts/shots.mjs <exe> --scan <ruta>` (escritorio oculto + CDP). Vite en :1420 para la versión de depuración.
- En la terminal Bash, los heredocs colapsan `\\` a `\`: editar código con barras invertidas con Edit/Write.
- UAC del usuario en «no notificar»: la elevación es silenciosa.
