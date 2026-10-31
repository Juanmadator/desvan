import { describe, expect, it } from "vitest";
import { apply, compose, easeInOutCubic, easeOutCubic, IDENTITY, rectTransform, zoomInXform, zoomOutXform, type Rect4 } from "./anim";

const close = (a: [number, number], b: [number, number]) => {
  expect(a[0]).toBeCloseTo(b[0], 6);
  expect(a[1]).toBeCloseTo(b[1], 6);
};

describe("anim", () => {
  it("suavizados con extremos fijos", () => {
    for (const f of [easeInOutCubic, easeOutCubic]) {
      expect(f(0)).toBe(0);
      expect(f(1)).toBe(1);
      expect(f(-3)).toBe(0);
      expect(f(7)).toBe(1);
    }
    expect(easeInOutCubic(0.5)).toBeCloseTo(0.5, 9);
  });

  it("rectTransform lleva un rectángulo sobre otro", () => {
    const x = rectTransform([10, 20, 30, 40], [0, 0, 300, 200]);
    close(apply(x, [10, 20]), [0, 0]);
    close(apply(x, [40, 60]), [300, 200]);
  });

  it("zoom hacia dentro: de quieto a llenar la vista", () => {
    const target: Rect4 = [100, 50, 200, 100];
    const full: Rect4 = [0, 0, 800, 600];
    const a = zoomInXform(target, full, 0);
    close(apply(a, [100, 50]), [100, 50]);
    close(apply(a, [300, 150]), [300, 150]);
    const b = zoomInXform(target, full, 1);
    close(apply(b, [100, 50]), [0, 0]);
    close(apply(b, [300, 150]), [800, 600]);
  });

  it("zoom hacia fuera: termina en la identidad", () => {
    const child: Rect4 = [400, 300, 100, 80];
    const full: Rect4 = [0, 0, 800, 600];
    const a = zoomOutXform(child, full, 0);
    close(apply(a, [400, 300]), [0, 0]);
    close(apply(a, [500, 380]), [800, 600]);
    const b = zoomOutXform(child, full, 1);
    close(apply(b, [123, 456]), [123, 456]);
  });

  it("compose aplica en orden", () => {
    const a = { sx: 2, sy: 3, tx: 1, ty: 1 };
    const b = { sx: 0.5, sy: 1, tx: -4, ty: 2 };
    close(apply(compose(a, b), [5, 7]), apply(b, apply(a, [5, 7])));
    expect(compose(IDENTITY, a)).toEqual(a);
  });
});
