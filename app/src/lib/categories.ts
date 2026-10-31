// Categorías de archivo: etiquetas y colores compartidos por el mapa, el árbol y las estadísticas.
// Los colores salen de la paleta del treemap para que todo cuadre.
import { CATEGORY_COLORS, CATEGORY_LABELS } from "./treemap/palette";

export function catColor(cat: number): string {
  if (cat === 255) return "#c9973f";
  return CATEGORY_COLORS[cat] ?? CATEGORY_COLORS[0];
}

export function catLabel(cat: number): string {
  if (cat === 255) return "Carpetas";
  return CATEGORY_LABELS[cat] ?? CATEGORY_LABELS[0];
}
