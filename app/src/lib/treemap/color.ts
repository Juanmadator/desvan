// Utilidades de color (sRGB en 0..1).

export type RGB = [number, number, number];

/** "#rgb", "#rrggbb" o "rgb(r, g, b)". Lo que no entiende sale gris medio. */
export function parseColor(c: string): RGB {
  const s = c.trim();
  if (s.startsWith("#")) {
    const h = s.slice(1);
    if (h.length === 3 && /^[0-9a-f]{3}$/i.test(h)) {
      return [0, 1, 2].map((i) => parseInt(h[i] + h[i], 16) / 255) as RGB;
    }
    if (h.length >= 6 && /^[0-9a-f]{6}/i.test(h)) {
      return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16) / 255) as RGB;
    }
  }
  const m = /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/i.exec(s);
  if (m) return [m[1], m[2], m[3]].map((v) => clamp01(parseFloat(v) / 255)) as RGB;
  return [0.5, 0.5, 0.5];
}

export function toHex(rgb: RGB): string {
  return "#" + rgb.map((v) => Math.round(clamp01(v) * 255).toString(16).padStart(2, "0")).join("");
}

export function cssRgba(rgb: RGB, a: number): string {
  const [r, g, b] = rgb.map((v) => Math.round(clamp01(v) * 255));
  return `rgba(${r}, ${g}, ${b}, ${clamp01(a)})`;
}

export function clamp01(v: number): number {
  return v < 0 ? 0 : v > 1 ? 1 : v;
}

export function srgbToLinear(c: number): number {
  return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
}

export function linearToSrgb(c: number): number {
  return c <= 0.0031308 ? c * 12.92 : 1.055 * Math.pow(c, 1 / 2.4) - 0.055;
}

/** Luminancia relativa (WCAG). */
export function luminance(rgb: RGB): number {
  const [r, g, b] = rgb.map(srgbToLinear);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** Contraste WCAG entre dos colores (1..21). */
export function contrast(a: RGB, b: RGB): number {
  const la = luminance(a);
  const lb = luminance(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

export function mix(a: RGB, b: RGB, t: number): RGB {
  return [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
}

export function scale(rgb: RGB, k: number): RGB {
  return [rgb[0] * k, rgb[1] * k, rgb[2] * k];
}

/** El texto (claro u oscuro) que mejor se lee sobre `bg`. */
export function readableOn(bg: RGB, light: string, dark: string): string {
  return contrast(bg, parseColor(light)) >= contrast(bg, parseColor(dark)) ? light : dark;
}

/** Paleta como uniforme vec3[n] (sRGB 0..1); los huecos repiten el primer color. */
export function paletteUniform(colors: readonly string[], size = 16): Float32Array {
  const out = new Float32Array(size * 3);
  const first = parseColor(colors[0] ?? "#808080");
  for (let i = 0; i < size; i++) {
    const c = i < colors.length ? parseColor(colors[i]) : first;
    out.set(c, i * 3);
  }
  return out;
}
