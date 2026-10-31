import { describe, expect, it } from "vitest";
import { contrast, cssRgba, linearToSrgb, luminance, mix, paletteUniform, parseColor, readableOn, srgbToLinear, toHex } from "./color";
import { CATEGORIES, CATEGORY_COLORS, CATEGORY_LABELS, THEME } from "./palette";

describe("color", () => {
  it("interpreta hex y rgb()", () => {
    expect(parseColor("#fff")).toEqual([1, 1, 1]);
    expect(toHex(parseColor("#0d0f14"))).toBe("#0d0f14");
    expect(toHex(parseColor("rgb(255, 128, 0)"))).toBe("#ff8000");
    expect(parseColor("no es un color")).toEqual([0.5, 0.5, 0.5]);
  });
  it("convierte entre sRGB y lineal", () => {
    for (const v of [0, 0.01, 0.2, 0.5, 0.99, 1]) expect(linearToSrgb(srgbToLinear(v))).toBeCloseTo(v, 6);
  });
  it("calcula contraste WCAG", () => {
    expect(contrast([0, 0, 0], [1, 1, 1])).toBeCloseTo(21, 5);
    expect(luminance([1, 1, 1])).toBeCloseTo(1, 6);
    expect(readableOn(parseColor("#e4b24d"), "#ffffff", "#10131a")).toBe("#10131a");
    expect(readableOn(parseColor("#3a4a8a"), "#ffffff", "#10131a")).toBe("#ffffff");
  });
  it("mezcla y formatea", () => {
    expect(mix([0, 0, 0], [1, 1, 1], 0.25)).toEqual([0.25, 0.25, 0.25]);
    expect(cssRgba([1, 0, 0], 0.5)).toBe("rgba(255, 0, 0, 0.5)");
  });
  it("prepara la paleta como uniforme", () => {
    const u = paletteUniform(["#ff0000", "#00ff00"], 4);
    expect(u.length).toBe(12);
    expect(Array.from(u.slice(0, 6))).toEqual([1, 0, 0, 0, 1, 0]);
    expect(Array.from(u.slice(9, 12))).toEqual([1, 0, 0]);
  });
});

describe("paleta", () => {
  it("tiene una entrada por categoría de Rust, en orden", () => {
    expect(CATEGORIES.length).toBe(12);
    CATEGORIES.forEach((c, i) => expect(c.code).toBe(i));
    expect(CATEGORY_COLORS.length).toBe(12);
    expect(CATEGORY_LABELS[1]).toBe("Vídeo");
    expect(new Set(CATEGORY_COLORS).size).toBe(12);
  });
  it("los colores se distinguen del fondo y entre sí", () => {
    const bg = parseColor(THEME.background);
    const cols = CATEGORY_COLORS.map(parseColor);
    for (const c of cols) expect(contrast(c, bg)).toBeGreaterThan(4);
    for (let i = 0; i < cols.length; i++) {
      for (let j = i + 1; j < cols.length; j++) {
        const d = Math.hypot(cols[i][0] - cols[j][0], cols[i][1] - cols[j][1], cols[i][2] - cols[j][2]);
        expect(d, `${CATEGORIES[i].id} / ${CATEGORIES[j].id}`).toBeGreaterThan(0.12);
      }
    }
  });
});
