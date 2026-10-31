import { describe, expect, it } from "vitest";
import { decodeLayout, encodeLayout, FIELD, isFrame, LAYOUT_HEADER_BYTES, rectInfo, RFLAG, STRIDE } from "./buffer";

function sample() {
  const rects = new Float32Array(2 * STRIDE);
  rects.set([0, 0, 100, 50, 1, -1, 1, -1, 1, 255, RFLAG.DIR | RFLAG.HEADER, -1], 0);
  rects.set([2, 18, 98, 48, 2, -2, 2, -2, 2, 1, 0, 0], STRIDE);
  const ids = new Uint32Array([7, 4_000_000_000]);
  return { rects, ids };
}

describe("decodeLayout", () => {
  it("lee lo que escribe encodeLayout", () => {
    const { rects, ids } = sample();
    const buf = encodeLayout(rects, ids, { root: 3, width: 100, height: 50, header: 18, padding: 2, metric: "size" });
    expect(buf.byteLength).toBe(LAYOUT_HEADER_BYTES + 2 * (STRIDE * 4 + 4));
    const d = decodeLayout(buf);
    expect(d.count).toBe(2);
    expect(d.meta).toMatchObject({ root: 3, width: 100, height: 50, header: 18, padding: 2, metric: "size", stride: STRIDE, version: 1 });
    expect(Array.from(d.rects)).toEqual(Array.from(rects));
    expect(Array.from(d.ids)).toEqual([7, 4_000_000_000]);
    // sin copia
    expect(d.rects.buffer).toBe(buf);
  });

  it("acepta vistas alineadas y desalineadas", () => {
    const { rects, ids } = sample();
    const raw = new Uint8Array(encodeLayout(rects, ids, { root: 1, width: 10, height: 10 }));
    for (const off of [0, 4, 3]) {
      const host = new Uint8Array(raw.length + off);
      host.set(raw, off);
      const d = decodeLayout(new Uint8Array(host.buffer, off, raw.length));
      expect(d.ids[1]).toBe(4_000_000_000);
      expect(d.rects[STRIDE + FIELD.X1]).toBe(98);
    }
  });

  it("rechaza búferes que no son un layout", () => {
    expect(() => decodeLayout(new ArrayBuffer(8))).toThrow();
    const bad = new Uint8Array(encodeLayout(sample().rects, sample().ids, { root: 1, width: 1, height: 1 }));
    bad[0] = 0;
    expect(() => decodeLayout(bad)).toThrow(/Desván/);
    const short = encodeLayout(sample().rects, sample().ids, { root: 1, width: 1, height: 1 }).slice(0, 60);
    expect(() => decodeLayout(short)).toThrow(/truncado/);
  });

  it("rectInfo da ancho, alto y campos", () => {
    const { rects, ids } = sample();
    const r = rectInfo(rects, ids, 1);
    expect(r).toMatchObject({ x: 2, y: 18, w: 96, h: 30, depth: 2, category: 1, flags: 0, parent: 0, id: 4_000_000_000 });
    expect(isFrame(rectInfo(rects, ids, 0).flags)).toBe(true);
    expect(isFrame(RFLAG.DIR | RFLAG.COLLAPSED)).toBe(false);
  });
});
