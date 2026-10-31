// Formato del búfer de rectángulos que produce `desvan_core::layout` (ver layout.rs).
// 12 f32 por rectángulo = tres vec4 por instancia:
//   0 x0  1 y0  2 x1  3 y1        bordes en px de la vista (origen arriba a la izquierda)
//   4 gx0 5 gx1 6 gy0 7 gy1       pendiente del cojín (dz/dx, dz/dy) en cada borde
//   8 depth 9 cat 10 flags 11 parent
// Orden en preorden: cada carpeta antes que su contenido.

export const STRIDE = 12;

export const FIELD = {
  X0: 0,
  Y0: 1,
  X1: 2,
  Y1: 3,
  GX0: 4,
  GX1: 5,
  GY0: 6,
  GY1: 7,
  DEPTH: 8,
  CAT: 9,
  FLAGS: 10,
  PARENT: 11,
} as const;

/** Bits del campo `flags` (rflags en Rust). */
export const RFLAG = {
  /** Carpeta (marco). */
  DIR: 1,
  /** Reserva arriba una franja de título de `meta.header` px. */
  HEADER: 2,
  /** Deja un margen de `meta.padding` px alrededor del contenido. */
  PADDED: 4,
  /** Carpeta sin desplegar: se pinta maciza con su categoría. */
  COLLAPSED: 8,
  /** Agrupa los hijos demasiado pequeños (su id es el de la carpeta). */
  REST: 16,
  /** Nodo sintético (huérfanos, espacio libre…). */
  SYNTHETIC: 32,
  /** Única carpeta hija visible de su padre: sin marco propio; su nombre va en la franja del padre. */
  CHAIN: 64,
} as const;

/** Categoría de una carpeta desplegada. */
export const CAT_DIR = 255;

export interface LayoutMeta {
  /** Nodo raíz de la vista. */
  root: number;
  /** Medidas (px CSS) para las que se calculó. */
  width: number;
  height: number;
  /** Alto de la franja de título de las carpetas con HEADER. */
  header?: number;
  /** Margen de las carpetas con PADDED. */
  padding?: number;
  metric?: "alloc" | "size";
}

export interface DecodedLayout {
  rects: Float32Array;
  ids: Uint32Array;
  count: number;
  meta: Required<LayoutMeta> & { version: number; stride: number };
}

// "DSVL" leído como u32 little-endian
export const LAYOUT_MAGIC = 0x4c565344;
export const LAYOUT_HEADER_BYTES = 40;

/**
 * Decodifica lo que devuelve `Layout::to_bytes` (little-endian):
 * "DSVL", u32 versión, u32 count, u32 stride, f32 ancho, f32 alto, u32 raíz,
 * f32 franja, f32 margen, u32 métrica; luego count·stride f32 y count u32.
 * Las vistas apuntan al mismo ArrayBuffer (sin copiar) si está alineado.
 */
export function decodeLayout(input: ArrayBuffer | ArrayBufferView): DecodedLayout {
  let buf: ArrayBuffer;
  let base = 0;
  if (input instanceof ArrayBuffer) {
    buf = input;
  } else {
    buf = input.buffer as ArrayBuffer;
    base = input.byteOffset;
    if (base % 4 !== 0) {
      // las vistas de f32 necesitan alineación de 4: se copia
      buf = new Uint8Array(input.buffer, input.byteOffset, input.byteLength).slice().buffer;
      base = 0;
    }
  }
  const len = input instanceof ArrayBuffer ? input.byteLength : input.byteLength;
  if (len < LAYOUT_HEADER_BYTES) throw new Error("treemap: búfer demasiado corto");
  const dv = new DataView(buf, base, len);
  if (dv.getUint32(0, true) !== LAYOUT_MAGIC) throw new Error("treemap: el búfer no es un layout de Desván");
  const version = dv.getUint32(4, true);
  if (version !== 1) throw new Error(`treemap: versión de layout ${version} no soportada`);
  const count = dv.getUint32(8, true);
  const stride = dv.getUint32(12, true);
  if (stride !== STRIDE) throw new Error(`treemap: stride ${stride} inesperado`);
  const need = LAYOUT_HEADER_BYTES + count * stride * 4 + count * 4;
  if (len < need) throw new Error("treemap: búfer truncado");
  const metric = dv.getUint32(36, true) === 1 ? "size" : "alloc";
  const meta = {
    version,
    stride,
    width: dv.getFloat32(16, true),
    height: dv.getFloat32(20, true),
    root: dv.getUint32(24, true),
    header: dv.getFloat32(28, true),
    padding: dv.getFloat32(32, true),
    metric,
  } as DecodedLayout["meta"];
  const rects = new Float32Array(buf, base + LAYOUT_HEADER_BYTES, count * stride);
  const ids = new Uint32Array(buf, base + LAYOUT_HEADER_BYTES + count * stride * 4, count);
  return { rects, ids, count, meta };
}

/** Codifica en el mismo formato (pruebas, laboratorio). */
export function encodeLayout(rects: Float32Array, ids: Uint32Array, meta: LayoutMeta): ArrayBuffer {
  const count = ids.length;
  const out = new ArrayBuffer(LAYOUT_HEADER_BYTES + count * STRIDE * 4 + count * 4);
  const dv = new DataView(out);
  dv.setUint32(0, LAYOUT_MAGIC, true);
  dv.setUint32(4, 1, true);
  dv.setUint32(8, count, true);
  dv.setUint32(12, STRIDE, true);
  dv.setFloat32(16, meta.width, true);
  dv.setFloat32(20, meta.height, true);
  dv.setUint32(24, meta.root >>> 0, true);
  dv.setFloat32(28, meta.header ?? 0, true);
  dv.setFloat32(32, meta.padding ?? 0, true);
  dv.setUint32(36, meta.metric === "size" ? 1 : 0, true);
  new Float32Array(out, LAYOUT_HEADER_BYTES, count * STRIDE).set(rects.subarray(0, count * STRIDE));
  new Uint32Array(out, LAYOUT_HEADER_BYTES + count * STRIDE * 4, count).set(ids.subarray(0, count));
  return out;
}

/** Datos de un rectángulo, ya con ancho y alto. */
export interface RectInfo {
  index: number;
  id: number;
  x: number;
  y: number;
  w: number;
  h: number;
  depth: number;
  category: number;
  flags: number;
  /** Índice del rectángulo padre (-1 = cuelga de la raíz). */
  parent: number;
}

export function rectInfo(rects: Float32Array, ids: Uint32Array, i: number): RectInfo {
  const o = i * STRIDE;
  return {
    index: i,
    id: ids[i],
    x: rects[o],
    y: rects[o + 1],
    w: rects[o + 2] - rects[o],
    h: rects[o + 3] - rects[o + 1],
    depth: rects[o + FIELD.DEPTH],
    category: rects[o + FIELD.CAT],
    flags: rects[o + FIELD.FLAGS],
    parent: rects[o + FIELD.PARENT],
  };
}

/** Es un marco de carpeta desplegada (no un bloque relleno). */
export function isFrame(flags: number): boolean {
  return (flags & RFLAG.DIR) !== 0 && (flags & RFLAG.COLLAPSED) === 0;
}
