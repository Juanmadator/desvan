// Índice espacial para saber qué hay bajo el cursor: rejilla uniforme en la que cada
// celda guarda (en orden creciente) los rectángulos que la tocan. Como el búfer va en
// preorden, el más profundo bajo un punto es el de índice mayor que lo contiene.

import { STRIDE } from "./buffer";

export class RectGrid {
  readonly cell: number;
  readonly cols: number;
  readonly rows: number;
  readonly count: number;
  readonly width: number;
  readonly height: number;
  private readonly rects: Float32Array;
  private readonly starts: Uint32Array;
  private readonly items: Uint32Array;

  constructor(rects: Float32Array, count: number, width: number, height: number, cell?: number) {
    this.rects = rects;
    this.count = count;
    this.width = width;
    this.height = height;
    const area = Math.max(1, width * height);
    // ~4 rectángulos de hoja por celda, entre 8 y 64 px
    this.cell = cell ?? Math.min(64, Math.max(8, Math.sqrt((area / Math.max(1, count)) * 4)));
    this.cols = Math.max(1, Math.ceil(width / this.cell));
    this.rows = Math.max(1, Math.ceil(height / this.cell));
    const n = this.cols * this.rows;
    const counts = new Uint32Array(n + 1);
    // 1ª pasada: cuántos por celda
    for (let i = 0; i < count; i++) {
      const [c0, r0, c1, r1] = this.span(i);
      for (let r = r0; r <= r1; r++) {
        const row = r * this.cols;
        for (let c = c0; c <= c1; c++) counts[row + c + 1]++;
      }
    }
    for (let k = 0; k < n; k++) counts[k + 1] += counts[k];
    this.starts = counts;
    this.items = new Uint32Array(counts[n]);
    const fill = counts.slice(0, n);
    // 2ª pasada: rellenar en orden de índice
    for (let i = 0; i < count; i++) {
      const [c0, r0, c1, r1] = this.span(i);
      for (let r = r0; r <= r1; r++) {
        const row = r * this.cols;
        for (let c = c0; c <= c1; c++) this.items[fill[row + c]++] = i;
      }
    }
  }

  /** Celdas [c0, r0, c1, r1] que toca el rectángulo i (vacío si cae fuera). */
  private span(i: number): [number, number, number, number] {
    const o = i * STRIDE;
    const x0 = this.rects[o];
    const y0 = this.rects[o + 1];
    const x1 = this.rects[o + 2];
    const y1 = this.rects[o + 3];
    if (!(x1 > x0 && y1 > y0) || x1 <= 0 || y1 <= 0 || x0 >= this.width || y0 >= this.height) return [1, 1, 0, 0];
    const c0 = Math.max(0, Math.floor(x0 / this.cell));
    const r0 = Math.max(0, Math.floor(y0 / this.cell));
    const c1 = Math.min(this.cols - 1, Math.floor(x1 / this.cell));
    const r1 = Math.min(this.rows - 1, Math.floor(y1 / this.cell));
    return [c0, r0, c1, r1];
  }

  /** Índice del rectángulo más profundo que contiene (x, y), o -1. */
  hit(x: number, y: number): number {
    if (!(x >= 0 && y >= 0 && x < this.width && y < this.height)) return -1;
    const c = Math.min(this.cols - 1, Math.floor(x / this.cell));
    const r = Math.min(this.rows - 1, Math.floor(y / this.cell));
    const k = r * this.cols + c;
    const rects = this.rects;
    for (let j = this.starts[k + 1] - 1; j >= this.starts[k]; j--) {
      const i = this.items[j];
      const o = i * STRIDE;
      if (x >= rects[o] && x < rects[o + 2] && y >= rects[o + 1] && y < rects[o + 3]) return i;
    }
    return -1;
  }

  /** Entradas totales (para medir el coste). */
  get size(): number {
    return this.items.length;
  }
}
