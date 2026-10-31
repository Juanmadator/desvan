import { describe, expect, it } from "vitest";
import { STRIDE, FIELD, RFLAG } from "./buffer";
import { RectGrid } from "./grid";

/** Búfer a partir de [x0, y0, x1, y1, flags, parent]. */
function buf(list: number[][]): Float32Array {
  const r = new Float32Array(list.length * STRIDE);
  list.forEach((v, i) => {
    r.set(v.slice(0, 4), i * STRIDE);
    r[i * STRIDE + FIELD.FLAGS] = v[4] ?? 0;
    r[i * STRIDE + FIELD.PARENT] = v[5] ?? -1;
  });
  return r;
}

function brute(r: Float32Array, n: number, x: number, y: number): number {
  for (let i = n - 1; i >= 0; i--) {
    const o = i * STRIDE;
    if (x >= r[o] && x < r[o + 2] && y >= r[o + 1] && y < r[o + 3]) return i;
  }
  return -1;
}

describe("RectGrid", () => {
  const rects = buf([
    [0, 0, 100, 100, RFLAG.DIR],
    [0, 0, 50, 100, 0, 0],
    [50, 0, 100, 100, RFLAG.DIR, 0],
    [55, 20, 95, 95, 0, 2],
  ]);

  it("devuelve el rectángulo más profundo", () => {
    const g = new RectGrid(rects, 4, 100, 100, 16);
    expect(g.hit(10, 10)).toBe(1);
    expect(g.hit(60, 10)).toBe(2); // franja de la carpeta
    expect(g.hit(60, 50)).toBe(3);
    expect(g.hit(50, 50)).toBe(2); // borde izquierdo incluido, derecho no
    expect(g.hit(99.9, 99.9)).toBe(2);
  });

  it("fuera de la vista no hay nada", () => {
    const g = new RectGrid(rects, 4, 100, 100);
    expect(g.hit(-1, 10)).toBe(-1);
    expect(g.hit(100, 10)).toBe(-1);
    expect(g.hit(10, 1e9)).toBe(-1);
    expect(g.hit(NaN, 3)).toBe(-1);
  });

  it("coincide con la búsqueda exhaustiva en una partición aleatoria", () => {
    // particiones recursivas tipo treemap, con márgenes
    let seed = 12345;
    const rnd = () => (seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff;
    const list: number[][] = [];
    const split = (x0: number, y0: number, x1: number, y1: number, depth: number, parent: number): void => {
      const i = list.length;
      const leaf = depth > 4 || x1 - x0 < 6 || y1 - y0 < 6;
      list.push([x0, y0, x1, y1, leaf ? 0 : RFLAG.DIR, parent]);
      if (leaf) return;
      const [a0, b0, a1, b1] = [x0 + 1, y0 + 1, x1 - 1, y1 - 1];
      const wide = x1 - x0 > y1 - y0;
      const lo = wide ? a0 : b0;
      const hi = wide ? a1 : b1;
      const k = 2 + Math.floor(rnd() * 3);
      let pos = lo;
      for (let j = 0; j < k; j++) {
        const end = j === k - 1 ? hi : Math.min(hi, pos + ((hi - lo) / k) * (0.6 + rnd() * 0.8));
        if (wide) split(pos, b0, end, b1, depth + 1, i);
        else split(a0, pos, a1, end, depth + 1, i);
        pos = end;
      }
    };
    split(0, 0, 640, 400, 0, -1);
    const r = buf(list);
    const g = new RectGrid(r, list.length, 640, 400);
    for (let t = 0; t < 4000; t++) {
      const x = rnd() * 640;
      const y = rnd() * 400;
      expect(g.hit(x, y)).toBe(brute(r, list.length, x, y));
    }
  });

  it("aguanta 300k rectángulos", () => {
    const n = 300_000;
    const r = new Float32Array(n * STRIDE);
    const cols = 600;
    for (let i = 0; i < n; i++) {
      const x = (i % cols) * 3.2;
      const y = Math.floor(i / cols) * 2.16;
      r.set([x, y, x + 3.2, y + 2.16], i * STRIDE);
    }
    const t0 = performance.now();
    const g = new RectGrid(r, n, 1920, 1080);
    const built = performance.now() - t0;
    expect(g.hit(1000.5, 500.5)).toBe(brute(r, n, 1000.5, 500.5));
    const t1 = performance.now();
    for (let k = 0; k < 10_000; k++) g.hit((k * 7.3) % 1920, (k * 3.1) % 1080);
    const perHit = (performance.now() - t1) / 10_000;
    expect(built).toBeLessThan(500);
    expect(perHit).toBeLessThan(0.05);
  });
});
