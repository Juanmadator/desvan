// Etiquetas: recorte con puntos suspensivos, elección de huecos y control de solapes.
// Lógica pura (la medición del texto se inyecta) para poder probarla sin navegador.

import { FIELD, RFLAG, STRIDE } from "./buffer";

export type Measure = (text: string) => number;
export const ELLIPSIS = "…";

/** Extensión final (".mkv") si parece una; "" si no. */
export function extOf(name: string): string {
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || dot === name.length - 1 || name.length - dot > 9) return "";
  const e = name.slice(dot);
  return /\s/.test(e) ? "" : e;
}

/**
 * Recorta `text` para que mida como mucho `max` px.
 * - "end": "Program Fi…"
 * - "middle": conserva el final y la extensión: "Grabación 20…07.mkv"
 * Devuelve null si no caben al menos `minChars` caracteres y los puntos.
 */
export function ellipsize(text: string, max: number, measure: Measure, mode: "end" | "middle" = "end", minChars = 2): string | null {
  if (!text || !(max > 0)) return null;
  if (measure(text) <= max) return text;
  const chars = Array.from(text);
  const n = chars.length;
  const ext = mode === "middle" ? extOf(text) : "";
  const extLen = Array.from(ext).length;
  const build = (k: number): string => {
    // k = caracteres conservados
    if (mode === "middle" && k >= extLen + 4) {
      const tail = Math.min(k - 2, extLen + 2);
      const head = k - tail;
      return chars.slice(0, head).join("").trimEnd() + ELLIPSIS + chars.slice(n - tail).join("");
    }
    return chars.slice(0, k).join("").trimEnd() + ELLIPSIS;
  };
  let lo = minChars;
  let hi = n - 1;
  if (lo > hi || measure(build(lo)) > max) return null;
  // el mayor k que cabe
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (measure(build(mid)) <= max) lo = mid;
    else hi = mid - 1;
  }
  return build(lo);
}

export type SlotKind = "header" | "block";

export interface LabelSlot {
  index: number;
  kind: SlotKind;
  /** Caja disponible en px CSS. */
  x: number;
  y: number;
  w: number;
  h: number;
  /** Área del rectángulo (para priorizar). */
  area: number;
}

export interface PlanOptions {
  /** Escala de px del layout a px CSS. */
  sx: number;
  sy: number;
  /** Franja de título y margen de las carpetas (px del layout). */
  header: number;
  padding: number;
  /** Bloque mínimo para rotular un archivo (px CSS). */
  minW: number;
  minH: number;
  /** Ancho mínimo de una franja para rotularla. */
  minHeaderW: number;
  /** Margen interior del texto. */
  inset: number;
  /** Tope de etiquetas. */
  max: number;
}

export const DEFAULT_PLAN: PlanOptions = {
  sx: 1,
  sy: 1,
  header: 0,
  padding: 0,
  minW: 44,
  minH: 16,
  minHeaderW: 28,
  inset: 6,
  max: 1500,
};

/** Huecos con sitio para texto: franjas de título de carpeta y bloques grandes. */
export function planLabels(rects: Float32Array, count: number, opts: Partial<PlanOptions> = {}): LabelSlot[] {
  const o = { ...DEFAULT_PLAN, ...opts };
  const out: LabelSlot[] = [];
  const headH = o.header * o.sy;
  for (let i = 0; i < count; i++) {
    const b = i * STRIDE;
    const flags = rects[b + FIELD.FLAGS];
    const x = rects[b] * o.sx;
    const y = rects[b + 1] * o.sy;
    const w = (rects[b + 2] - rects[b]) * o.sx;
    const h = (rects[b + 3] - rects[b + 1]) * o.sy;
    if (!(w > 0 && h > 0)) continue;
    const frame = (flags & RFLAG.DIR) !== 0 && (flags & RFLAG.COLLAPSED) === 0;
    if (frame) {
      if ((flags & RFLAG.HEADER) !== 0 && headH > 0 && w - 2 * o.inset >= o.minHeaderW) {
        out.push({ index: i, kind: "header", x: x + o.inset, y, w: w - 2 * o.inset, h: headH, area: w * h });
      }
    } else if (w >= o.minW && h >= o.minH) {
      const ins = Math.min(o.inset, w * 0.1);
      out.push({ index: i, kind: "block", x: x + ins, y: y + 1, w: w - 2 * ins, h: h - 2, area: w * h });
    }
  }
  // primero lo grande; a igual área, el orden del búfer
  out.sort((a, b) => b.area - a.area || a.index - b.index);
  if (out.length > o.max) out.length = o.max;
  return out;
}

/** Cajas ya ocupadas por texto: garantiza que dos etiquetas nunca se pisan. */
export class LabelOccupancy {
  private readonly cell: number;
  private readonly cols: number;
  private readonly rows: number;
  private readonly cells: number[][];
  private readonly boxes: [number, number, number, number][] = [];

  constructor(width: number, height: number, cell = 48) {
    this.cell = cell;
    this.cols = Math.max(1, Math.ceil(width / cell));
    this.rows = Math.max(1, Math.ceil(height / cell));
    this.cells = Array.from({ length: this.cols * this.rows }, () => []);
  }

  private range(x: number, y: number, w: number, h: number): [number, number, number, number] {
    const c0 = Math.max(0, Math.floor(x / this.cell));
    const r0 = Math.max(0, Math.floor(y / this.cell));
    const c1 = Math.min(this.cols - 1, Math.floor((x + w) / this.cell));
    const r1 = Math.min(this.rows - 1, Math.floor((y + h) / this.cell));
    return [c0, r0, c1, r1];
  }

  /** ¿Está libre la caja? */
  free(x: number, y: number, w: number, h: number): boolean {
    const [c0, r0, c1, r1] = this.range(x, y, w, h);
    for (let r = r0; r <= r1; r++) {
      for (let c = c0; c <= c1; c++) {
        for (const k of this.cells[r * this.cols + c]) {
          const [bx, by, bw, bh] = this.boxes[k];
          if (x < bx + bw && bx < x + w && y < by + bh && by < y + h) return false;
        }
      }
    }
    return true;
  }

  /** Ocupa la caja si está libre; devuelve si se pudo. */
  place(x: number, y: number, w: number, h: number): boolean {
    if (!this.free(x, y, w, h)) return false;
    const k = this.boxes.length;
    this.boxes.push([x, y, w, h]);
    const [c0, r0, c1, r1] = this.range(x, y, w, h);
    for (let r = r0; r <= r1; r++) for (let c = c0; c <= c1; c++) this.cells[r * this.cols + c].push(k);
    return true;
  }

  get size(): number {
    return this.boxes.length;
  }
}

/** Caché de medidas de texto por fuente. */
export class MeasureCache {
  private readonly map = new Map<string, number>();
  private readonly limit: number;
  constructor(limit = 50_000) {
    this.limit = limit;
  }
  get(font: string, text: string, measure: (t: string) => number): number {
    const key = font + "\u0000" + text;
    let w = this.map.get(key);
    if (w === undefined) {
      if (this.map.size >= this.limit) this.map.clear();
      w = measure(text);
      this.map.set(key, w);
    }
    return w;
  }
  clear(): void {
    this.map.clear();
  }
}
