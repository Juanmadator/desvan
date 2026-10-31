import { describe, expect, it } from "vitest";
import { bytes, count, duration, pct } from "./format";

describe("formatos", () => {
  it("tamaños en base 1024 con coma decimal", () => {
    expect(bytes(0)).toBe("0 B");
    expect(bytes(1536)).toBe("1,50 KB");
    expect(bytes(10 * 1024 ** 3)).toBe("10,0 GB");
    expect(bytes(750 * 1024 ** 3)).toBe("750 GB");
  });
  it("números, porcentajes y duraciones", () => {
    expect(count(1234567)).toBe("1.234.567");
    expect(pct(0.256)).toBe("25,6 %");
    expect(pct(0.0004)).toBe("<0,1 %");
    expect(duration(850)).toBe("850 ms");
    expect(duration(1830)).toBe("1,83 s");
  });
});
