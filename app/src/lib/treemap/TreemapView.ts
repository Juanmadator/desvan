// Vista del treemap con WebGL2: un quad instanciado por rectángulo, sombreado de cojín
// en el sombreador de fragmentos y etiquetas en un lienzo 2D superpuesto.
//
// La imagen base se pinta en una textura (solo cuando cambian datos, tamaño, paleta,
// resaltado o durante una animación). Mover el ratón solo recompone esa textura y
// dibuja los contornos, así que el coste por fotograma no depende del nº de rectángulos.

import { FIELD, RFLAG, STRIDE, isFrame, rectInfo, type LayoutMeta, type RectInfo } from "./buffer";
import { CATEGORY_COLORS, THEME } from "./palette";
import { RectGrid } from "./grid";
import { ellipsize, LabelOccupancy, MeasureCache, planLabels, type LabelSlot } from "./labels";
import { contrast, cssRgba, parseColor, paletteUniform, type RGB } from "./color";
import { compose, easeInOutCubic, IDENTITY, zoomInXform, zoomOutXform, type Rect4, type Xform } from "./anim";
import { BASE_FS, BASE_VS, COMPOSITE_FS, COMPOSITE_VS, OUTLINE_FS, OUTLINE_VS } from "./shaders";

export interface TreemapHit {
  index: number;
  id: number;
}

export interface TreemapLabel {
  name: string;
  /** Alternativa corta si `name` no cabe entera (p. ej. «29.721» por «29.721 elementos»). */
  short?: string;
  /** Segunda línea o texto a la derecha de la franja (p. ej. el tamaño). */
  detail?: string;
}

export type TreemapTheme = { -readonly [K in keyof typeof THEME]: string };

export interface TreemapOptions {
  /** Color por categoría (por defecto la paleta de palette.ts). */
  palette?: string[];
  theme?: Partial<TreemapTheme>;
  /** Etiquetas sobre el mapa (por defecto sí, si hay `labelFor`). */
  labels?: boolean;
  /** Texto de un rectángulo; solo se pide para los que tienen sitio. */
  labelFor?: (index: number, id: number) => TreemapLabel | null | undefined;
  /** Duración del zoom (ms). */
  zoomDuration?: number;
  /** Seguir el tamaño del lienzo con un ResizeObserver (por defecto sí). */
  autoResize?: boolean;
  /** Iluminación de los cojines: dirección hacia la luz, luz ambiente y brillo especular. */
  light?: [number, number, number];
  ambient?: number;
  specular?: number;
  onHover?: TreemapView["onHover"];
  onClick?: TreemapView["onClick"];
  onDoubleClick?: TreemapView["onDoubleClick"];
  onContextMenu?: TreemapView["onContextMenu"];
}

interface Target {
  fb: WebGLFramebuffer;
  tex: WebGLTexture;
}

interface ZoomAnim {
  kind: "in" | "out";
  /** Rectángulo (px del layout) que crece o encoge. */
  rect: Rect4;
  start: number;
  dur: number;
  then: (() => void) | null;
}

type Uniforms = Record<string, WebGLUniformLocation | null>;
type ThemeRGB = Record<"background" | "panel" | "header" | "border" | "accent" | "hover", RGB>;

const OUTLINE_FLOATS = 12;
/** Ganancia del sombreado (la misma en el sombreador y al elegir el color del texto). */
const GAIN = 1.08;
const CHAIN_SEP = " › ";
const MAX_OUTLINES = 4;

export class TreemapView {
  onHover: ((hit: TreemapHit | null, ev: MouseEvent | null) => void) | null = null;
  onClick: ((hit: TreemapHit | null, ev: MouseEvent) => void) | null = null;
  onDoubleClick: ((hit: TreemapHit | null, ev: MouseEvent) => void) | null = null;
  /** Si hay manejador se evita el menú del navegador. */
  onContextMenu: ((hit: TreemapHit | null, ev: MouseEvent) => void) | null = null;

  readonly canvas: HTMLCanvasElement;
  /** Lienzo 2D de las etiquetas, encima del de WebGL. */
  readonly overlay: HTMLCanvasElement;

  private gl!: WebGL2RenderingContext;
  private readonly ctx: CanvasRenderingContext2D;
  private theme: TreemapTheme;
  private themeRGB: ThemeRGB;
  private paletteColors: string[];
  private paletteF32: Float32Array;
  private paletteRGB: RGB[] = [];
  private labelFor: TreemapOptions["labelFor"] | null;
  private labelsOn: boolean;
  private zoomDuration: number;
  private light: [number, number, number];
  private ambient: number;
  private specular: number;

  // datos
  private rects: Float32Array<ArrayBufferLike> = new Float32Array(0);
  private ids: Uint32Array<ArrayBufferLike> = new Uint32Array(0);
  private n = 0;
  private meta: Required<LayoutMeta> = { root: 0, width: 1, height: 1, header: 0, padding: 0, metric: "alloc" };
  private marks = new Uint8Array(0);
  private grid: RectGrid | null = null;
  private pred: ((index: number) => boolean) | null = null;

  // WebGL
  private progBase!: WebGLProgram;
  private progComp!: WebGLProgram;
  private progLine!: WebGLProgram;
  private uBase: Uniforms = {};
  private uComp: Uniforms = {};
  private uLine: Uniforms = {};
  private vaoBase!: WebGLVertexArrayObject;
  private vaoLine!: WebGLVertexArrayObject;
  private bufRects!: WebGLBuffer;
  private bufMarks!: WebGLBuffer;
  private bufLine!: WebGLBuffer;
  private targets: Target[] = [];
  private cur = 0;
  private lineData = new Float32Array(MAX_OUTLINES * OUTLINE_FLOATS);

  // tamaño
  private cssW = 1;
  private cssH = 1;
  private dpr = 1;
  private devW = 1;
  private devH = 1;

  // estado
  private hover = -1;
  private selectedId: number | null = null;
  private selected = -1;
  private hiT = 1;
  private hiStart = 0;
  private readonly hiDur = 180;
  private zoom: ZoomAnim | null = null;
  private pending: { kind: "in" } | { kind: "out"; from: number } | null = null;
  private fadeStart = 0;
  private fadeDur = 0;
  private baseDirty = true;
  private labelsDirty = true;
  private raf = 0;
  private hoverRaf = 0;
  private pendingTimer = 0;
  private lastPointer: { x: number; y: number; ev: MouseEvent } | null = null;
  private lost = false;
  private disposed = false;
  private readonly measureCache = new MeasureCache();
  private observer: ResizeObserver | null = null;
  private mql: MediaQueryList | null = null;
  private readonly listeners: [EventTarget, string, EventListener][] = [];

  constructor(canvas: HTMLCanvasElement, opts: TreemapOptions = {}) {
    this.canvas = canvas;
    this.theme = { ...THEME, ...opts.theme };
    this.themeRGB = themeColors(this.theme);
    this.paletteColors = opts.palette ?? CATEGORY_COLORS;
    this.paletteF32 = paletteUniform(this.paletteColors);
    this.paletteRGB = this.paletteColors.map(parseColor);
    this.labelFor = opts.labelFor ?? null;
    this.labelsOn = opts.labels ?? true;
    this.zoomDuration = opts.zoomDuration ?? 260;
    this.light = normalize3(opts.light ?? [-1, -1.25, 2.4]);
    this.ambient = opts.ambient ?? 0.2;
    this.specular = opts.specular ?? 0;
    this.onHover = opts.onHover ?? null;
    this.onClick = opts.onClick ?? null;
    this.onDoubleClick = opts.onDoubleClick ?? null;
    this.onContextMenu = opts.onContextMenu ?? null;

    const ov = document.createElement("canvas");
    ov.className = "treemap-labels";
    ov.setAttribute("aria-hidden", "true");
    Object.assign(ov.style, {
      position: "absolute",
      left: "0px",
      top: "0px",
      pointerEvents: "none",
      transition: "opacity 140ms ease",
    } satisfies Partial<CSSStyleDeclaration>);
    canvas.insertAdjacentElement("afterend", ov);
    const parent = canvas.parentElement;
    if (parent && getComputedStyle(parent).position === "static") parent.style.position = "relative";
    this.overlay = ov;
    const ctx = ov.getContext("2d");
    if (!ctx) throw new Error("treemap: sin contexto 2D");
    this.ctx = ctx;

    this.initGL();
    this.bindEvents(opts.autoResize ?? true);
    this.resize();
  }

  // ---------- API ----------

  /** Datos de `desvan_core::layout` (ver buffer.ts). No se copian: no los modifiques después. */
  setData(rects: Float32Array, ids: Uint32Array, meta: LayoutMeta): void {
    const n = Math.min(ids.length, Math.floor(rects.length / STRIDE));
    this.rects = rects;
    this.ids = ids;
    this.n = n;
    this.meta = {
      root: meta.root,
      width: meta.width > 0 ? meta.width : this.cssW,
      height: meta.height > 0 ? meta.height : this.cssH,
      header: meta.header ?? 0,
      padding: meta.padding ?? 0,
      metric: meta.metric ?? "alloc",
    };
    this.marks = new Uint8Array(n * 2);
    for (let i = 0; i < n; i++) {
      const v = this.pred ? (this.pred(i) ? 255 : 0) : 255;
      this.marks[2 * i] = v;
      this.marks[2 * i + 1] = v;
    }
    this.hiT = 1;
    this.upload();
    this.grid = new RectGrid(rects, n, this.meta.width, this.meta.height);
    this.hover = -1;
    this.selected = this.selectedId === null ? -1 : this.indexOfId(this.selectedId);

    const now = performance.now();
    const p = this.pending;
    this.pending = null;
    clearTimeout(this.pendingTimer);
    if (p) {
      // la imagen anterior (la del zoom o la vista de la que se sube) pasa a «prev»
      this.cur = 1 - this.cur;
      if (p.kind === "out") {
        const i = this.indexOfId(p.from);
        if (i >= 0) {
          const r = this.rectAt(i);
          this.zoom = { kind: "out", rect: [r.x, r.y, r.w, r.h], start: now, dur: this.zoomDuration, then: null };
          this.startFade(now, this.zoomDuration * 0.55);
        } else {
          this.startFade(now, 200);
        }
      } else {
        this.startFade(now, 170);
      }
    }
    this.baseDirty = true;
    this.labelsDirty = true;
    this.request();
  }

  /** Colores por categoría (hex). */
  setPalette(colors: string[]): void {
    this.paletteColors = colors.slice();
    this.paletteF32 = paletteUniform(this.paletteColors);
    this.paletteRGB = this.paletteColors.map(parseColor);
    this.baseDirty = true;
    this.labelsDirty = true;
    this.request();
  }

  /** Resalta lo que cumpla `pred` (por índice) y apaga el resto; null lo quita. */
  setHighlight(pred: ((index: number) => boolean) | null): void {
    this.pred = pred;
    const m = this.marks;
    for (let i = 0; i < this.n; i++) {
      m[2 * i] = m[2 * i + 1];
      m[2 * i + 1] = pred ? (pred(i) ? 255 : 0) : 255;
    }
    if (!this.lost) {
      const gl = this.gl;
      gl.bindBuffer(gl.ARRAY_BUFFER, this.bufMarks);
      gl.bufferData(gl.ARRAY_BUFFER, m, gl.DYNAMIC_DRAW);
    }
    this.hiStart = performance.now();
    this.hiT = 0;
    this.baseDirty = true;
    this.labelsDirty = true;
    this.request();
  }

  setSelected(id: number | null): void {
    this.selectedId = id;
    this.selected = id === null ? -1 : this.indexOfId(id);
    this.request();
  }

  setLabelProvider(fn: TreemapOptions["labelFor"] | null): void {
    this.labelFor = fn;
    this.labelsDirty = true;
    this.request();
  }

  setLabelsVisible(on: boolean): void {
    this.labelsOn = on;
    this.labelsDirty = true;
    this.request();
  }

  /**
   * Zoom animado. Con `fromRect` (px del layout, [x, y, ancho, alto]) ese rectángulo
   * crece hasta llenar la vista y al terminar se llama a `then`, que debe llamar a
   * `setData` con la carpeta nueva (ya o cuando llegue). Con null (subir) se llama
   * a `then` enseguida y, al llegar los datos del padre, la vista anterior encoge
   * hasta su sitio dentro de ellos.
   */
  zoomTo(fromRect: [number, number, number, number] | null, then: () => void): void {
    if (this.disposed) return;
    this.hover = -1;
    // las etiquetas desaparecen al instante y vuelven con un fundido al terminar
    this.overlay.style.transition = "none";
    this.overlay.style.opacity = "0";
    if (fromRect && this.n > 0 && fromRect[2] > 0 && fromRect[3] > 0) {
      this.zoom = { kind: "in", rect: [...fromRect], start: performance.now(), dur: this.zoomDuration, then };
      this.request();
      return;
    }
    // sin animación de entrada: fundido al llegar los datos
    this.pending = fromRect ? { kind: "in" } : { kind: "out", from: this.meta.root };
    this.armPendingTimeout();
    this.request();
    then();
  }

  /** El rectángulo más profundo bajo (x, y) en px CSS relativos al lienzo. */
  pick(x: number, y: number): TreemapHit | null {
    if (!this.grid || this.n === 0) return null;
    const lx = (x * this.meta.width) / this.cssW;
    const ly = (y * this.meta.height) / this.cssH;
    const i = this.grid.hit(lx, ly);
    return i < 0 ? null : { index: i, id: this.ids[i] };
  }

  /** Índice del rectángulo de un nodo (sin contar «restos»), o -1. */
  indexOfId(id: number): number {
    const r = this.rects;
    for (let i = 0; i < this.n; i++) {
      if (this.ids[i] === id && (r[i * STRIDE + FIELD.FLAGS] & RFLAG.REST) === 0) return i;
    }
    return -1;
  }

  rectAt(index: number): RectInfo {
    return rectInfo(this.rects, this.ids, index);
  }

  /** Rectángulo en px CSS del lienzo (para colocar menús o tooltips). */
  screenRect(index: number): Rect4 {
    const r = this.rectAt(index);
    const kx = this.cssW / this.meta.width;
    const ky = this.cssH / this.meta.height;
    return [r.x * kx, r.y * ky, r.w * kx, r.h * ky];
  }

  get count(): number {
    return this.n;
  }

  get hovered(): number {
    return this.hover;
  }

  /** Hay una animación en curso (zoom, fundido o resaltado). */
  get busy(): boolean {
    return this.zoom !== null || this.pending !== null || this.fadeDur > 0 || this.hiT < 1;
  }

  /** Vuelve a medir el lienzo (lo hace solo si `autoResize`). */
  resize(): void {
    if (this.disposed) return;
    const cw = this.canvas.clientWidth || this.canvas.width || 1;
    const ch = this.canvas.clientHeight || this.canvas.height || 1;
    const dpr = window.devicePixelRatio || 1;
    const dw = Math.max(1, Math.round(cw * dpr));
    const dh = Math.max(1, Math.round(ch * dpr));
    this.overlay.style.left = `${this.canvas.offsetLeft}px`;
    this.overlay.style.top = `${this.canvas.offsetTop}px`;
    this.overlay.style.width = `${cw}px`;
    this.overlay.style.height = `${ch}px`;
    if (dw === this.devW && dh === this.devH && cw === this.cssW && ch === this.cssH && dpr === this.dpr && this.targets.length) return;
    this.cssW = cw;
    this.cssH = ch;
    this.dpr = dpr;
    this.devW = dw;
    this.devH = dh;
    this.canvas.width = dw;
    this.canvas.height = dh;
    this.overlay.width = dw;
    this.overlay.height = dh;
    if (!this.lost) this.allocTargets();
    this.baseDirty = true;
    this.labelsDirty = true;
    this.request();
  }

  /** Fuerza un repintado completo. */
  render(): void {
    this.baseDirty = true;
    this.labelsDirty = true;
    this.request();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    if (this.raf) cancelAnimationFrame(this.raf);
    if (this.hoverRaf) cancelAnimationFrame(this.hoverRaf);
    clearTimeout(this.pendingTimer);
    this.observer?.disconnect();
    this.mql?.removeEventListener("change", this.onDprChange);
    for (const [t, type, fn] of this.listeners) t.removeEventListener(type, fn);
    if (!this.lost) {
      const gl = this.gl;
      for (const t of this.targets) {
        gl.deleteFramebuffer(t.fb);
        gl.deleteTexture(t.tex);
      }
      gl.deleteBuffer(this.bufRects);
      gl.deleteBuffer(this.bufMarks);
      gl.deleteBuffer(this.bufLine);
      gl.deleteVertexArray(this.vaoBase);
      gl.deleteVertexArray(this.vaoLine);
      gl.deleteProgram(this.progBase);
      gl.deleteProgram(this.progComp);
      gl.deleteProgram(this.progLine);
      gl.getExtension("WEBGL_lose_context")?.loseContext();
    }
    this.targets = [];
    this.overlay.remove();
  }

  // ---------- WebGL ----------

  private initGL(): void {
    const gl = this.canvas.getContext("webgl2", {
      alpha: false,
      antialias: false,
      depth: false,
      stencil: false,
      premultipliedAlpha: true,
      preserveDrawingBuffer: false,
      powerPreference: "high-performance",
    });
    if (!gl) throw new Error("treemap: WebGL2 no disponible");
    this.gl = gl;
    this.progBase = program(gl, BASE_VS, BASE_FS);
    this.progComp = program(gl, COMPOSITE_VS, COMPOSITE_FS);
    this.progLine = program(gl, OUTLINE_VS, OUTLINE_FS);
    this.uBase = uniforms(gl, this.progBase, [
      "u_xform",
      "u_view",
      "u_snap",
      "u_palette",
      "u_light",
      "u_ambient",
      "u_gain",
      "u_spec",
      "u_bg",
      "u_panel",
      "u_header",
      "u_border",
      "u_headerPx",
      "u_dpr",
      "u_hiT",
    ]);
    this.uComp = uniforms(gl, this.progComp, ["u_cur", "u_prev", "u_fade"]);
    this.uLine = uniforms(gl, this.progLine, ["u_view"]);

    // rectángulos: tres vec4 por instancia + vec2 de resaltado
    this.vaoBase = must(gl.createVertexArray());
    gl.bindVertexArray(this.vaoBase);
    this.bufRects = must(gl.createBuffer());
    gl.bindBuffer(gl.ARRAY_BUFFER, this.bufRects);
    for (let k = 0; k < 3; k++) {
      gl.enableVertexAttribArray(k);
      gl.vertexAttribPointer(k, 4, gl.FLOAT, false, STRIDE * 4, k * 16);
      gl.vertexAttribDivisor(k, 1);
    }
    this.bufMarks = must(gl.createBuffer());
    gl.bindBuffer(gl.ARRAY_BUFFER, this.bufMarks);
    gl.enableVertexAttribArray(3);
    gl.vertexAttribPointer(3, 2, gl.UNSIGNED_BYTE, true, 2, 0);
    gl.vertexAttribDivisor(3, 1);

    // contornos
    this.vaoLine = must(gl.createVertexArray());
    gl.bindVertexArray(this.vaoLine);
    this.bufLine = must(gl.createBuffer());
    gl.bindBuffer(gl.ARRAY_BUFFER, this.bufLine);
    gl.bufferData(gl.ARRAY_BUFFER, this.lineData.byteLength, gl.DYNAMIC_DRAW);
    for (let k = 0; k < 3; k++) {
      gl.enableVertexAttribArray(k);
      gl.vertexAttribPointer(k, 4, gl.FLOAT, false, OUTLINE_FLOATS * 4, k * 16);
      gl.vertexAttribDivisor(k, 1);
    }
    gl.bindVertexArray(null);
    this.targets = [];
    this.allocTargets();
  }

  private allocTargets(): void {
    const gl = this.gl;
    for (const t of this.targets) {
      gl.deleteFramebuffer(t.fb);
      gl.deleteTexture(t.tex);
    }
    this.targets = [0, 1].map(() => {
      const tex = must(gl.createTexture());
      gl.bindTexture(gl.TEXTURE_2D, tex);
      gl.texStorage2D(gl.TEXTURE_2D, 1, gl.RGBA8, this.devW, this.devH);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      const fb = must(gl.createFramebuffer());
      gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
      const bg = parseColor(this.theme.background);
      gl.clearColor(bg[0], bg[1], bg[2], 1);
      gl.clear(gl.COLOR_BUFFER_BIT);
      return { fb, tex };
    });
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  }

  private upload(): void {
    if (this.lost) return;
    const gl = this.gl;
    gl.bindBuffer(gl.ARRAY_BUFFER, this.bufRects);
    gl.bufferData(gl.ARRAY_BUFFER, this.rects.subarray(0, this.n * STRIDE), gl.STATIC_DRAW);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.bufMarks);
    gl.bufferData(gl.ARRAY_BUFFER, this.marks, gl.DYNAMIC_DRAW);
  }

  private request(): void {
    if (!this.raf && !this.disposed) this.raf = requestAnimationFrame(this.frame);
  }

  /** Si `then` nunca llama a setData, la vista vuelve a su estado normal a los 4 s. */
  private armPendingTimeout(): void {
    clearTimeout(this.pendingTimer);
    this.pendingTimer = window.setTimeout(() => {
      if (!this.pending || this.disposed) return;
      this.pending = null;
      this.baseDirty = true;
      this.labelsDirty = true;
      this.request();
    }, 4000);
  }

  private startFade(now: number, dur: number): void {
    this.fadeStart = now;
    this.fadeDur = dur;
  }

  /** Layout -> px de dispositivo, sin zoom. */
  private baseXform(): Xform {
    return { sx: this.devW / this.meta.width, sy: this.devH / this.meta.height, tx: 0, ty: 0 };
  }

  private readonly frame = (now: number): void => {
    this.raf = 0;
    if (this.lost || this.disposed || !this.targets.length) return;
    let again = false;
    let view: Xform = IDENTITY;
    let moving = false;
    let finishedIn: ZoomAnim | null = null;

    const z = this.zoom;
    if (z) {
      const t = Math.min(1, (now - z.start) / z.dur);
      const e = easeInOutCubic(t);
      const full: Rect4 = [0, 0, this.meta.width, this.meta.height];
      view = z.kind === "in" ? zoomInXform(z.rect, full, e) : zoomOutXform(z.rect, full, e);
      moving = true;
      this.baseDirty = true;
      if (t >= 1) {
        this.zoom = null;
        if (z.kind === "in") {
          finishedIn = z;
        } else {
          view = IDENTITY;
          moving = false;
          this.labelsDirty = true;
        }
      } else {
        again = true;
      }
    }

    if (this.hiT < 1) {
      this.hiT = Math.min(1, (now - this.hiStart) / this.hiDur);
      this.baseDirty = true;
      if (this.hiT < 1) again = true;
    }

    let fade = 1;
    if (this.fadeDur > 0) {
      fade = Math.min(1, (now - this.fadeStart) / this.fadeDur);
      if (fade >= 1) {
        this.fadeDur = 0;
        this.labelsDirty = true;
      } else {
        again = true;
      }
    }

    // mientras se esperan los datos de un zoom se mantiene la última imagen
    if (this.baseDirty && !this.pending) {
      this.renderBase(compose(view, this.baseXform()), !moving);
      this.baseDirty = moving && !finishedIn;
    }
    this.composite(fade);
    const busy = moving || finishedIn !== null || this.fadeDur > 0 || this.pending !== null;
    if (!busy) this.drawOutlines();
    if (this.labelsDirty && !busy) {
      this.labelsDirty = false;
      this.drawLabels();
      if (this.overlay.style.opacity !== "1") {
        this.overlay.style.transition = "opacity 160ms ease";
        this.overlay.style.opacity = "1";
      }
    }
    if (finishedIn) {
      // la carpeta ya llena la vista: ahora los datos nuevos
      this.pending = { kind: "in" };
      this.armPendingTimeout();
      const then = finishedIn.then;
      if (then) then();
    }
    if (again) this.request();
  };

  private renderBase(xf: Xform, snap: boolean): void {
    const gl = this.gl;
    const t = this.targets[this.cur];
    gl.bindFramebuffer(gl.FRAMEBUFFER, t.fb);
    gl.viewport(0, 0, this.devW, this.devH);
    const c = this.themeRGB;
    gl.clearColor(c.background[0], c.background[1], c.background[2], 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    if (this.n > 0) {
      const u = this.uBase;
      gl.useProgram(this.progBase);
      gl.uniform4f(u.u_xform, xf.sx, xf.sy, xf.tx, xf.ty);
      gl.uniform2f(u.u_view, this.devW, this.devH);
      gl.uniform1f(u.u_snap, snap ? 1 : 0);
      gl.uniform3fv(u.u_palette, this.paletteF32);
      gl.uniform3fv(u.u_light, this.light);
      gl.uniform1f(u.u_ambient, this.ambient);
      gl.uniform1f(u.u_gain, GAIN);
      gl.uniform1f(u.u_spec, this.specular);
      gl.uniform3fv(u.u_bg, c.background);
      gl.uniform3fv(u.u_panel, c.panel);
      gl.uniform3fv(u.u_header, c.header);
      gl.uniform3fv(u.u_border, c.border);
      gl.uniform1f(u.u_headerPx, this.meta.header * xf.sy);
      gl.uniform1f(u.u_dpr, this.dpr);
      gl.uniform1f(u.u_hiT, easeInOutCubic(this.hiT));
      gl.bindVertexArray(this.vaoBase);
      gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, this.n);
      gl.bindVertexArray(null);
    }
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  }

  private composite(fade: number): void {
    const gl = this.gl;
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.viewport(0, 0, this.devW, this.devH);
    gl.useProgram(this.progComp);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.targets[this.cur].tex);
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, this.targets[1 - this.cur].tex);
    gl.uniform1i(this.uComp.u_cur, 0);
    gl.uniform1i(this.uComp.u_prev, 1);
    gl.uniform1f(this.uComp.u_fade, easeInOutCubic(fade));
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  }

  private drawOutlines(): void {
    const out: number[][] = [];
    const accent = this.themeRGB.accent;
    const white = this.themeRGB.hover;
    const d = this.dpr;
    const h = this.hover;
    if (h >= 0 && h < this.n) {
      const p = this.rects[h * STRIDE + FIELD.PARENT];
      if (p >= 0 && p !== this.selected) out.push([...this.devBox(p), ...white, 0.32, d, 0, 0, 0]);
    }
    if (this.selected >= 0 && this.selected < this.n) {
      out.push([...this.devBox(this.selected), ...accent, 1, 2 * d, 9 * d, 0.62, 0.12]);
    }
    if (h >= 0 && h < this.n && h !== this.selected) {
      out.push([...this.devBox(h), ...white, 0.95, 1.5 * d, 0, 0, 0.1]);
    }
    if (!out.length) return;
    const data = this.lineData;
    out.slice(0, MAX_OUTLINES).forEach((v, k) => data.set(v, k * OUTLINE_FLOATS));
    const gl = this.gl;
    gl.useProgram(this.progLine);
    gl.uniform2f(this.uLine.u_view, this.devW, this.devH);
    gl.bindVertexArray(this.vaoLine);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.bufLine);
    gl.bufferSubData(gl.ARRAY_BUFFER, 0, data);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, Math.min(out.length, MAX_OUTLINES));
    gl.disable(gl.BLEND);
    gl.bindVertexArray(null);
  }

  /** Caja del rectángulo i en px de dispositivo, ajustada al píxel como la base. */
  private devBox(i: number): [number, number, number, number] {
    const o = i * STRIDE;
    const xf = this.baseXform();
    const r = this.rects;
    return [
      Math.round(r[o] * xf.sx),
      Math.round(r[o + 1] * xf.sy),
      Math.round(r[o + 2] * xf.sx),
      Math.round(r[o + 3] * xf.sy),
    ];
  }

  // ---------- etiquetas ----------

  private drawLabels(): void {
    const ctx = this.ctx;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, this.overlay.width, this.overlay.height);
    if (!this.labelsOn || !this.labelFor || this.n === 0) return;
    ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
    const sx = this.cssW / this.meta.width;
    const sy = this.cssH / this.meta.height;
    const slots = planLabels(this.rects, this.n, { sx, sy, header: this.meta.header, padding: this.meta.padding });
    const occ = new LabelOccupancy(this.cssW, this.cssH);
    ctx.textBaseline = "middle";
    for (const s of slots) {
      const lab = this.labelFor(s.index, this.ids[s.index]);
      if (!lab || !lab.name) continue;
      const dim = this.pred !== null && !this.pred(s.index);
      ctx.globalAlpha = dim ? 0.3 : 1;
      if (s.kind === "header") this.drawHeaderLabel(s, lab, occ);
      else this.drawBlockLabel(s, lab, occ);
    }
    ctx.globalAlpha = 1;
  }

  private font(weight: number, size: number): string {
    return `${weight} ${size}px ${this.theme.fontFamily}`;
  }

  private measure(font: string, text: string): number {
    const ctx = this.ctx;
    return this.measureCache.get(font, text, (t) => {
      ctx.font = font;
      return ctx.measureText(t).width;
    });
  }

  private drawHeaderLabel(s: LabelSlot, lab: TreemapLabel, occ: LabelOccupancy): void {
    const ctx = this.ctx;
    const nameFont = this.font(600, 12);
    const detFont = this.font(400, 11);
    const cy = s.y + s.h / 2 + 0.5;
    const mName = (t: string) => this.measure(nameFont, t);
    const wDet = lab.detail ? this.measure(detFont, lab.detail) : 0;
    const gap = 10;
    const full = this.chainName(s.index, lab.name);
    const fit = (w: number) => this.fitChain(full, w, mName);
    let name: string | null;
    let showDet = false;
    if (lab.detail && mName(full) + gap + wDet <= s.w) {
      name = full;
      showDet = true;
    } else if (lab.detail && s.w >= wDet + gap + 56) {
      name = fit(s.w - wDet - gap);
      showDet = true;
    } else {
      name = fit(s.w);
    }
    if (!name) return;
    const wName = mName(name);
    if (!occ.place(s.x, cy - 7, wName, 14)) return;
    ctx.font = nameFont;
    ctx.textAlign = "left";
    ctx.fillStyle = this.theme.text;
    ctx.fillText(name, s.x, cy);
    if (showDet && lab.detail && occ.place(s.x + s.w - wDet, cy - 7, wDet, 14)) {
      ctx.font = detFont;
      ctx.textAlign = "right";
      ctx.fillStyle = this.theme.textDim;
      ctx.fillText(lab.detail, s.x + s.w, cy);
    }
  }

  private drawBlockLabel(s: LabelSlot, lab: TreemapLabel, occ: LabelOccupancy): void {
    const ctx = this.ctx;
    const flags = this.rects[s.index * STRIDE + FIELD.FLAGS];
    const cat = this.rects[s.index * STRIDE + FIELD.CAT];
    const big = s.w >= 150 && s.h >= 52;
    const size = big ? 13 : 12;
    const nameFont = this.font(600, size);
    const detFont = this.font(400, 11);
    const isFile = (flags & (RFLAG.DIR | RFLAG.REST)) === 0;
    const mName = (t: string) => this.measure(nameFont, t);
    // un nombre recortado a 3 o 4 letras no dice nada: mejor sin etiqueta
    const name =
      mName(lab.name) > s.w && lab.short && mName(lab.short) <= s.w
        ? lab.short
        : ellipsize(lab.name, s.w, mName, isFile ? "middle" : "end", isFile ? 6 : 5);
    if (!name) return;
    const two = !!lab.detail && s.h >= 36;
    const cx = s.x + s.w / 2;
    const cy = s.y + s.h / 2;
    const yName = two ? cy - 7 : cy;
    const wName = mName(name);
    if (!occ.place(cx - wName / 2, yName - size / 2 - 1, wName, size + 2)) return;
    // color real bajo el texto (con el sombreado del cojín en el centro)
    const bg = this.shadedCenter(s.index, cat, flags);
    const lightText = contrast(bg, [1, 1, 1]) >= 0.8 * contrast(bg, parseColor(this.theme.textDark));
    const fg = lightText ? this.theme.text : this.theme.textDark;
    const shadow = lightText ? "rgba(0, 0, 0, 0.45)" : "rgba(255, 255, 255, 0.22)";
    ctx.textAlign = "center";
    ctx.font = nameFont;
    ctx.fillStyle = shadow;
    ctx.fillText(name, cx, yName + 1);
    ctx.fillStyle = fg;
    ctx.fillText(name, cx, yName);
    if (two && lab.detail) {
      // el tamaño se ve entero o no se ve
      const det = this.measure(detFont, lab.detail) <= s.w ? lab.detail : null;
      if (det) {
        const wDet = this.measure(detFont, det);
        if (occ.place(cx - wDet / 2, cy + 8 - 7, wDet, 14)) {
          ctx.font = detFont;
          ctx.fillStyle = shadow;
          ctx.fillText(det, cx, cy + 9);
          ctx.fillStyle = lightText ? cssRgba(parseColor(this.theme.text), 0.78) : cssRgba(parseColor(this.theme.textDark), 0.78);
          ctx.fillText(det, cx, cy + 8);
        }
      }
    }
  }

  /** Color sombreado en el centro de un bloque, como lo calcula BASE_FS. */
  private shadedCenter(i: number, cat: number, flags: number): RGB {
    const o = i * STRIDE;
    const r = this.rects;
    let base = this.paletteRGB[cat] ?? this.paletteRGB[0] ?? ([0.5, 0.5, 0.5] as RGB);
    if (flags & (RFLAG.REST | RFLAG.COLLAPSED | RFLAG.SYNTHETIC)) {
      const l = 0.299 * base[0] + 0.587 * base[1] + 0.114 * base[2];
      base = flags & RFLAG.SYNTHETIC ? [l * 0.7, l * 0.7, l * 0.7] : (base.map((c) => (l + (c - l) * 0.7) * 0.92) as RGB);
    }
    const gx = (r[o + FIELD.GX0] + r[o + FIELD.GX1]) / 2;
    const gy = (r[o + FIELD.GY0] + r[o + FIELD.GY1]) / 2;
    const [lx, ly, lz] = this.light;
    const d = Math.max(0, (-gx * lx - gy * ly + lz) / Math.hypot(gx, gy, 1));
    const k = (this.ambient + (1 - this.ambient) * d) * GAIN;
    return base.map((c) => Math.min(1, c * k)) as RGB;
  }

  /** Nombre de una carpeta seguido de su cadena de hijas únicas: «Users › usuario». */
  private chainName(i: number, first: string): string {
    let name = first;
    let prev = i;
    for (let k = i + 1; k < this.n; k++) {
      const o = k * STRIDE;
      if (this.rects[o + FIELD.PARENT] !== prev || (this.rects[o + FIELD.FLAGS] & RFLAG.CHAIN) === 0) break;
      const lab = this.labelFor?.(k, this.ids[k]);
      if (!lab?.name) break;
      name += CHAIN_SEP + lab.name;
      prev = k;
    }
    return name;
  }

  /** Encaja una cadena: completa, «primera › … › última» o recortada. */
  private fitChain(full: string, w: number, measure: (t: string) => number): string | null {
    if (measure(full) <= w) return full;
    const parts = full.split(CHAIN_SEP);
    if (parts.length >= 3) {
      const short = parts[0] + CHAIN_SEP + "…" + CHAIN_SEP + parts[parts.length - 1];
      if (measure(short) <= w) return short;
    }
    return ellipsize(full, w, measure, "end");
  }

  // ---------- eventos ----------

  private bindEvents(autoResize: boolean): void {
    const c = this.canvas;
    const on = (t: EventTarget, type: string, fn: EventListener) => {
      t.addEventListener(type, fn);
      this.listeners.push([t, type, fn]);
    };
    on(c, "mousemove", (e) => {
      const ev = e as MouseEvent;
      this.lastPointer = { x: ev.offsetX, y: ev.offsetY, ev };
      if (!this.hoverRaf) this.hoverRaf = requestAnimationFrame(this.processHover);
    });
    on(c, "mouseleave", (e) => {
      this.lastPointer = null;
      if (this.hover !== -1) {
        this.hover = -1;
        this.request();
        this.onHover?.(null, e as MouseEvent);
      }
    });
    const hitOf = (e: Event) => {
      const ev = e as MouseEvent;
      return this.zoom || this.pending ? null : this.pick(ev.offsetX, ev.offsetY);
    };
    on(c, "click", (e) => this.onClick?.(hitOf(e), e as MouseEvent));
    on(c, "dblclick", (e) => this.onDoubleClick?.(hitOf(e), e as MouseEvent));
    on(c, "contextmenu", (e) => {
      if (!this.onContextMenu) return;
      e.preventDefault();
      this.onContextMenu(hitOf(e), e as MouseEvent);
    });
    on(c, "webglcontextlost", (e) => {
      e.preventDefault();
      this.lost = true;
      if (this.raf) cancelAnimationFrame(this.raf);
      this.raf = 0;
    });
    on(c, "webglcontextrestored", () => {
      this.lost = false;
      this.initGL();
      this.upload();
      this.render();
    });
    if (autoResize && typeof ResizeObserver !== "undefined") {
      this.observer = new ResizeObserver(() => this.resize());
      this.observer.observe(c);
    }
    this.watchDpr();
  }

  private readonly onDprChange = (): void => {
    this.resize();
    this.watchDpr();
  };

  private watchDpr(): void {
    this.mql?.removeEventListener("change", this.onDprChange);
    if (typeof matchMedia === "undefined") return;
    this.mql = matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
    this.mql.addEventListener("change", this.onDprChange);
  }

  private readonly processHover = (): void => {
    this.hoverRaf = 0;
    const p = this.lastPointer;
    if (!p || this.zoom || this.pending || this.fadeDur > 0) return;
    const hit = this.pick(p.x, p.y);
    const idx = hit ? hit.index : -1;
    if (idx !== this.hover) {
      this.hover = idx;
      this.request();
      this.onHover?.(hit, p.ev);
    }
  };
}

/** Es un marco de carpeta (re-exportado por comodidad). */
export { isFrame };

function themeColors(t: TreemapTheme): ThemeRGB {
  return {
    background: parseColor(t.background),
    panel: parseColor(t.panel),
    header: parseColor(t.header),
    border: parseColor(t.border),
    accent: parseColor(t.accent),
    hover: parseColor(t.hover),
  };
}

function normalize3(v: [number, number, number]): [number, number, number] {
  const l = Math.hypot(v[0], v[1], v[2]) || 1;
  return [v[0] / l, v[1] / l, v[2] / l];
}

function must<T>(v: T | null): T {
  if (v === null) throw new Error("treemap: no se pudo crear un recurso de WebGL");
  return v;
}

function shader(gl: WebGL2RenderingContext, type: number, src: string): WebGLShader {
  const s = must(gl.createShader(type));
  gl.shaderSource(s, src);
  gl.compileShader(s);
  if (!gl.getShaderParameter(s, gl.COMPILE_STATUS) && !gl.isContextLost()) {
    const log = gl.getShaderInfoLog(s);
    gl.deleteShader(s);
    throw new Error(`treemap: error al compilar el sombreador: ${log}`);
  }
  return s;
}

function program(gl: WebGL2RenderingContext, vs: string, fs: string): WebGLProgram {
  const p = must(gl.createProgram());
  const v = shader(gl, gl.VERTEX_SHADER, vs);
  const f = shader(gl, gl.FRAGMENT_SHADER, fs);
  gl.attachShader(p, v);
  gl.attachShader(p, f);
  gl.linkProgram(p);
  gl.deleteShader(v);
  gl.deleteShader(f);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS) && !gl.isContextLost()) {
    throw new Error(`treemap: error al enlazar: ${gl.getProgramInfoLog(p)}`);
  }
  return p;
}

function uniforms(gl: WebGL2RenderingContext, p: WebGLProgram, names: string[]): Uniforms {
  const u: Uniforms = {};
  for (const n of names) u[n] = gl.getUniformLocation(p, n);
  return u;
}
