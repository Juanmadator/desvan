// Animación: curvas de suavizado y transformaciones de la vista para el zoom.

/** [x, y, ancho, alto] */
export type Rect4 = [number, number, number, number];

/** Escala y desplazamiento: p' = p·s + t. */
export interface Xform {
  sx: number;
  sy: number;
  tx: number;
  ty: number;
}

export const IDENTITY: Xform = { sx: 1, sy: 1, tx: 0, ty: 0 };

export function clamp01(t: number): number {
  return t < 0 ? 0 : t > 1 ? 1 : t;
}

export function easeInOutCubic(t: number): number {
  t = clamp01(t);
  return t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
}

export function easeOutCubic(t: number): number {
  t = clamp01(t);
  return 1 - Math.pow(1 - t, 3);
}

export function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

export function lerpRect(a: Rect4, b: Rect4, t: number): Rect4 {
  return [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t), lerp(a[3], b[3], t)];
}

/** Transformación que lleva el rectángulo `from` exactamente sobre `to`. */
export function rectTransform(from: Rect4, to: Rect4): Xform {
  const sx = from[2] > 0 ? to[2] / from[2] : 1;
  const sy = from[3] > 0 ? to[3] / from[3] : 1;
  return { sx, sy, tx: to[0] - from[0] * sx, ty: to[1] - from[1] * sy };
}

/** Compone: primero `a`, después `b`. */
export function compose(a: Xform, b: Xform): Xform {
  return { sx: a.sx * b.sx, sy: a.sy * b.sy, tx: a.tx * b.sx + b.tx, ty: a.ty * b.sy + b.ty };
}

export function apply(x: Xform, p: [number, number]): [number, number] {
  return [p[0] * x.sx + x.tx, p[1] * x.sy + x.ty];
}

/**
 * Zoom hacia dentro: en t=0 el rectángulo pulsado está donde estaba; en t=1 llena la
 * vista. El rectángulo crece de forma lineal (con suavizado) en pantalla.
 */
export function zoomInXform(target: Rect4, full: Rect4, t: number): Xform {
  return rectTransform(target, lerpRect(target, full, t));
}

/** Zoom hacia fuera: en t=0 `child` (de los datos nuevos) llena la vista; en t=1 todo en su sitio. */
export function zoomOutXform(child: Rect4, full: Rect4, t: number): Xform {
  return rectTransform(child, lerpRect(full, child, t));
}
