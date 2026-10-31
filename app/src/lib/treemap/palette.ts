// Paleta del treemap y etiquetas de las categorías (mismo orden que
// crates/core/src/categories.rs). La leyenda y el resto de la interfaz la reutilizan.
// Colores elegidos en OKLCH con luminosidad y croma parejos, para que el sombreado
// de cojín los trate igual: vivos pero no fosforitos sobre el fondo oscuro.

export interface CategoryInfo {
  /** Código numérico (índice). */
  code: number;
  /** Id estable, igual que en Rust. */
  id: string;
  label: string;
  color: string;
}

export const CATEGORIES: readonly CategoryInfo[] = [
  { code: 0, id: "otros", label: "Otros", color: "#a39d96" },
  { code: 1, id: "video", label: "Vídeo", color: "#967de8" },
  { code: 2, id: "imagen", label: "Imágenes", color: "#33bda5" },
  { code: 3, id: "audio", label: "Audio", color: "#e070a5" },
  { code: 4, id: "comprimido", label: "Comprimidos e imágenes de disco", color: "#e4b24d" },
  { code: 5, id: "documento", label: "Documentos", color: "#5094e5" },
  { code: 6, id: "codigo", label: "Código y texto", color: "#68c36d" },
  { code: 7, id: "programa", label: "Programas y librerías", color: "#e6715b" },
  { code: 8, id: "sistema", label: "Sistema, temporales y logs", color: "#597f9f" },
  { code: 9, id: "juego", label: "Juegos y recursos", color: "#31bdda" },
  { code: 10, id: "datos", label: "Bases de datos y modelos de IA", color: "#b5ca4e" },
  { code: 11, id: "diseno", label: "Diseño, 3D y fuentes", color: "#c372c7" },
];

/** Color por código de categoría (hex). Las carpetas usan el código 255 (CAT_DIR). */
export const CATEGORY_COLORS: string[] = CATEGORIES.map((c) => c.color);
/** Etiqueta en español por código de categoría. */
export const CATEGORY_LABELS: string[] = CATEGORIES.map((c) => c.label);
export const CATEGORY_IDS: string[] = CATEGORIES.map((c) => c.id);

/** Tema oscuro por defecto del treemap. */
export const THEME = {
  background: "#0d0f14",
  /** Relleno de los marcos de carpeta (se ve en márgenes y huecos). */
  panel: "#12151c",
  /** Franja de título de las carpetas. */
  header: "#1a1f29",
  /** Borde de 1 px de las carpetas. */
  border: "#262c38",
  text: "#e8ebf1",
  textDim: "#8e97a8",
  /** Texto sobre colores claros. */
  textDark: "#10131a",
  hover: "#ffffff",
  accent: "#8fb3ff",
  fontFamily: '"Segoe UI Variable Text", "Segoe UI", system-ui, -apple-system, sans-serif',
} as const;
