import { describe, expect, it } from "vitest";
import { FIELD, RFLAG, STRIDE } from "./buffer";
import { ellipsize, extOf, LabelOccupancy, MeasureCache, planLabels } from "./labels";

const mono = (t: string) => Array.from(t).length * 7;

describe("ellipsize", () => {
  it("deja el texto si cabe", () => {
    expect(ellipsize("hola mundo", 70, mono)).toBe("hola mundo");
  });
  it("recorta por el final sin dejar espacios antes de los puntos", () => {
    expect(ellipsize("hola mundo", 50, mono)).toBe("hola m…");
    expect(ellipsize("hola mundo", 42, mono)).toBe("hola…");
    const r = ellipsize("Program Files (x86)", 60, mono)!;
    expect(mono(r)).toBeLessThanOrEqual(60);
    expect(r.endsWith("…")).toBe(true);
  });
  it("en medio conserva la extensión", () => {
    const r = ellipsize("Grabación 2026-03-14 21-33-07.mkv", 140, mono, "middle")!;
    expect(r).toBe("Grabación 202…07.mkv");
    expect(mono(r)).toBeLessThanOrEqual(140);
  });
  it("si no cabe ni lo mínimo devuelve null", () => {
    expect(ellipsize("hola mundo", 10, mono)).toBeNull();
    expect(ellipsize("", 100, mono)).toBeNull();
    expect(ellipsize("abc", 0, mono)).toBeNull();
  });
  it("respeta caracteres compuestos", () => {
    const r = ellipsize("\u{1F3AE}\u{1F3AE}\u{1F3AE}\u{1F3AE}\u{1F3AE}\u{1F3AE}", 30, mono)!;
    expect(r).toBe("\u{1F3AE}\u{1F3AE}\u{1F3AE}…");
  });
  it("siempre cabe en el ancho pedido", () => {
    const names = ["a.txt", "pagefile.sys", "Un nombre de carpeta bastante largo", "x".repeat(200) + ".mp4"];
    for (const n of names) {
      for (let w = 0; w < 300; w += 13) {
        for (const mode of ["end", "middle"] as const) {
          const r = ellipsize(n, w, mono, mode);
          if (r !== null) expect(mono(r)).toBeLessThanOrEqual(w);
        }
      }
    }
  });
});

describe("extOf", () => {
  it("reconoce extensiones razonables", () => {
    expect(extOf("peli.mkv")).toBe(".mkv");
    expect(extOf(".gitignore")).toBe("");
    expect(extOf("sin extension")).toBe("");
    expect(extOf("raro.una extension")).toBe("");
    expect(extOf("archivo.")).toBe("");
  });
});

function rects(list: [number, number, number, number, number][]): Float32Array {
  const r = new Float32Array(list.length * STRIDE);
  list.forEach(([x0, y0, x1, y1, f], i) => {
    r.set([x0, y0, x1, y1], i * STRIDE);
    r[i * STRIDE + FIELD.FLAGS] = f;
  });
  return r;
}

describe("planLabels", () => {
  const r = rects([
    [0, 0, 400, 300, RFLAG.DIR | RFLAG.HEADER | RFLAG.PADDED], // carpeta con franja
    [2, 18, 300, 298, 0], // archivo grande
    [300, 18, 398, 60, 0], // archivo mediano
    [300, 60, 330, 70, 0], // demasiado pequeño
    [330, 60, 398, 298, RFLAG.DIR], // carpeta sin franja: no se rotula
    [340, 70, 390, 120, RFLAG.REST], // resto con sitio
  ]);

  it("elige franjas y bloques con sitio, de mayor a menor", () => {
    const s = planLabels(r, 6, { header: 18, padding: 2 });
    expect(s.map((x) => [x.index, x.kind])).toEqual([
      [0, "header"],
      [1, "block"],
      [2, "block"],
      [5, "block"],
    ]);
    expect(s[0].h).toBe(18);
    expect(s[0].x).toBeGreaterThan(0);
  });

  it("aplica la escala a px CSS y el tope", () => {
    const s = planLabels(r, 6, { header: 18, sx: 0.5, sy: 0.5, max: 2 });
    expect(s.length).toBe(2);
    expect(s[0].h).toBe(9);
    expect(s[1].w).toBeLessThan(150);
  });
});

describe("LabelOccupancy", () => {
  it("no deja que dos etiquetas se pisen", () => {
    const o = new LabelOccupancy(500, 300, 32);
    expect(o.place(10, 10, 100, 14)).toBe(true);
    expect(o.place(60, 15, 100, 14)).toBe(false);
    expect(o.place(110, 10, 50, 14)).toBe(true); // justo al lado
    expect(o.place(10, 24, 100, 14)).toBe(true); // justo debajo
    expect(o.place(400, 200, 200, 200)).toBe(true); // se sale de la rejilla
    expect(o.size).toBe(4);
  });
});

describe("MeasureCache", () => {
  it("mide una sola vez por fuente y texto", () => {
    const c = new MeasureCache(3);
    let calls = 0;
    const m = (t: string) => {
      calls++;
      return t.length;
    };
    expect(c.get("12px a", "hola", m)).toBe(4);
    expect(c.get("12px a", "hola", m)).toBe(4);
    expect(c.get("13px a", "hola", m)).toBe(4);
    expect(calls).toBe(2);
  });
});
