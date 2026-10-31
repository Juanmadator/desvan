//! Treemap «squarified» (Bruls, Huizing y van Wijk, 2000) sobre el [`Tree`], con el
//! sombreado de cojín de van Wijk y van de Wetering (1999).
//!
//! # Búfer de salida
//!
//! [`Layout::rects`] es un búfer plano listo para subir a WebGL2 como datos por
//! instancia: [`STRIDE`] (= 12) `f32` por rectángulo (48 bytes), tres `vec4`:
//!
//! | i  | campo    | significado |
//! |----|----------|-------------|
//! | 0  | `x0`     | borde izquierdo (px de la vista, origen arriba a la izquierda) |
//! | 1  | `y0`     | borde superior |
//! | 2  | `x1`     | borde derecho |
//! | 3  | `y1`     | borde inferior |
//! | 4  | `gx0`    | pendiente del cojín `dz/dx` en el borde izquierdo |
//! | 5  | `gx1`    | `dz/dx` en el borde derecho |
//! | 6  | `gy0`    | `dz/dy` en el borde superior |
//! | 7  | `gy1`    | `dz/dy` en el borde inferior |
//! | 8  | `depth`  | profundidad (los hijos de la raíz son 1) |
//! | 9  | `cat`    | categoría ([`crate::categories`]); [`CAT_DIR`] = carpeta desplegada |
//! | 10 | `flags`  | bits de [`rflags`] |
//! | 11 | `parent` | índice del rectángulo padre en este búfer (-1 = cuelga de la raíz) |
//!
//! Se guardan bordes y no ancho/alto para que dos vecinos compartan exactamente el
//! mismo `f32`: al rasterizar (o ajustar a píxeles) no quedan rendijas.
//!
//! El orden es preorden: cada carpeta va antes que su contenido. Así se pinta de
//! atrás adelante y, al buscar bajo el cursor, el más profundo es el de índice mayor.
//! `ids` es paralelo a `rects` (id de nodo; en un «resto», el de su carpeta).
//!
//! # Cojines
//!
//! La superficie de cada rectángulo es la suma de las parábolas («crestas») suyas y de
//! sus antepasados. Su gradiente es lineal dentro del rectángulo, así que basta con el
//! valor en los bordes: el sombreador interpola y calcula
//! `n = normalize(-dz/dx, -dz/dy, 1)`.
//!
//! # Coste
//!
//! Solo se recorre lo visible: los hijos vienen ordenados de mayor a menor, y en cuanto
//! uno no llega a `min_area` el resto de la carpeta se agrupa en un rectángulo «resto».

use crate::categories::{self, OTROS, SISTEMA};
use crate::tree::{flags, NodeId, Tree, NONE};

/// `f32` por rectángulo.
pub const STRIDE: usize = 12;
/// Categoría de una carpeta desplegada (marco).
pub const CAT_DIR: u8 = 255;
/// Lado mínimo (px) del contenido de una carpeta para descender.
const MIN_SIDE: f64 = 2.0;

/// Índices de cada campo dentro de un rectángulo.
pub mod field {
    pub const X0: usize = 0;
    pub const Y0: usize = 1;
    pub const X1: usize = 2;
    pub const Y1: usize = 3;
    pub const GX0: usize = 4;
    pub const GX1: usize = 5;
    pub const GY0: usize = 6;
    pub const GY1: usize = 7;
    pub const DEPTH: usize = 8;
    pub const CAT: usize = 9;
    pub const FLAGS: usize = 10;
    pub const PARENT: usize = 11;
}

/// Bits del campo `flags`.
pub mod rflags {
    /// Carpeta (marco).
    pub const DIR: u32 = 1 << 0;
    /// La carpeta reserva arriba una franja de título de `Layout::header` px.
    pub const HEADER: u32 = 1 << 1;
    /// La carpeta deja un margen de `Layout::padding` px alrededor del contenido.
    pub const PADDED: u32 = 1 << 2;
    /// Carpeta sin desplegar (pequeña o en `max_depth`): se pinta maciza con `cat`.
    pub const COLLAPSED: u32 = 1 << 3;
    /// Agrupa los hijos demasiado pequeños de una carpeta (su id es el de la carpeta).
    pub const REST: u32 = 1 << 4;
    /// Nodo sintético del árbol (huérfanos, espacio libre…).
    pub const SYNTHETIC: u32 = 1 << 5;
}

/// Qué tamaño manda en las áreas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Metric {
    /// Tamaño en disco (clústeres asignados).
    #[default]
    Alloc,
    /// Tamaño lógico.
    Size,
}

impl Metric {
    /// "alloc"/"disk" o "size"/"logical".
    pub fn parse(s: &str) -> Option<Metric> {
        match s {
            "alloc" | "disk" => Some(Metric::Alloc),
            "size" | "logical" => Some(Metric::Size),
            _ => None,
        }
    }
    fn values(self, t: &Tree) -> &[u64] {
        match self {
            Metric::Alloc => &t.alloc,
            Metric::Size => &t.size,
        }
    }
    fn code(self) -> u32 {
        match self {
            Metric::Alloc => 0,
            Metric::Size => 1,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LayoutOpts {
    pub metric: Metric,
    /// Área mínima (px²) para dibujar algo por separado; lo menor se agrupa en un «resto».
    pub min_area: f32,
    /// Profundidad máxima (los hijos de la raíz son 1). Las carpetas a esa altura salen macizas.
    pub max_depth: u32,
    /// Margen interior de las carpetas (px), cuando caben.
    pub dir_padding: f32,
    /// Alto de la franja de título de las carpetas grandes (px); 0 = sin franjas.
    pub header: f32,
    /// Altura de la cresta del primer nivel (h de van Wijk).
    pub cushion_height: f32,
    /// Factor de la altura por nivel (f de van Wijk).
    pub cushion_falloff: f32,
    /// Tope orientativo de rectángulos: si hace falta sube `min_area` (0 = sin tope).
    pub max_rects: usize,
}

impl Default for LayoutOpts {
    fn default() -> Self {
        Self {
            metric: Metric::Alloc,
            min_area: 4.0,
            max_depth: 64,
            dir_padding: 2.0,
            header: 18.0,
            cushion_height: 0.5,
            cushion_falloff: 0.8,
            max_rects: 750_000,
        }
    }
}

/// Resultado del treemap. Ver la documentación del módulo para el formato.
#[derive(Clone, Debug)]
pub struct Layout {
    pub rects: Vec<f32>,
    pub ids: Vec<u32>,
    pub count: usize,
    pub width: f32,
    pub height: f32,
    pub root: NodeId,
    pub metric: Metric,
    /// Alto de la franja de título de las carpetas con [`rflags::HEADER`].
    pub header: f32,
    /// Margen de las carpetas con [`rflags::PADDED`].
    pub padding: f32,
    /// `min_area` aplicada (puede ser mayor que la pedida por `max_rects`).
    pub min_area: f32,
    scratch: Scratch,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            rects: Vec::new(),
            ids: Vec::new(),
            count: 0,
            width: 0.0,
            height: 0.0,
            root: NONE,
            metric: Metric::Alloc,
            header: 0.0,
            padding: 0.0,
            min_area: 0.0,
            scratch: Scratch::default(),
        }
    }
}

/// Cabecera del formato binario de [`Layout::to_bytes`].
pub const BYTES_MAGIC: [u8; 4] = *b"DSVL";
pub const BYTES_VERSION: u32 = 1;
pub const BYTES_HEADER: usize = 40;

impl Layout {
    pub fn len(&self) -> usize {
        self.count
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Los `STRIDE` valores del rectángulo `i`.
    pub fn record(&self, i: usize) -> &[f32] {
        &self.rects[i * STRIDE..(i + 1) * STRIDE]
    }
    /// `[x, y, ancho, alto]` del rectángulo `i`.
    pub fn rect(&self, i: usize) -> [f32; 4] {
        let r = self.record(i);
        [r[0], r[1], r[2] - r[0], r[3] - r[1]]
    }
    pub fn flags(&self, i: usize) -> u32 {
        self.rects[i * STRIDE + field::FLAGS] as u32
    }
    pub fn depth(&self, i: usize) -> u32 {
        self.rects[i * STRIDE + field::DEPTH] as u32
    }
    pub fn category(&self, i: usize) -> u8 {
        self.rects[i * STRIDE + field::CAT] as u8
    }
    /// Índice del rectángulo padre.
    pub fn parent(&self, i: usize) -> Option<usize> {
        let p = self.rects[i * STRIDE + field::PARENT];
        (p >= 0.0).then_some(p as usize)
    }
    /// Índice del rectángulo de un nodo (no cuenta los «restos»).
    pub fn find(&self, id: NodeId) -> Option<usize> {
        (0..self.count).find(|&i| self.ids[i] == id && self.flags(i) & rflags::REST == 0)
    }
    /// Rectángulos con sitio para una etiqueta: franjas de título y bloques de al menos
    /// `min_w` × `min_h` px. Sirve para pedir solo esos nombres al backend.
    pub fn label_candidates(&self, min_w: f32, min_h: f32) -> Vec<u32> {
        (0..self.count)
            .filter(|&i| {
                let f = self.flags(i);
                let [_, _, w, h] = self.rect(i);
                if f & rflags::DIR != 0 && f & rflags::COLLAPSED == 0 {
                    f & rflags::HEADER != 0
                } else {
                    w >= min_w && h >= min_h
                }
            })
            .map(|i| i as u32)
            .collect()
    }

    /// Serializa para enviarlo a la interfaz (little-endian):
    ///
    /// | bytes | campo |
    /// |-------|-------|
    /// | 0..4   | `"DSVL"` |
    /// | 4..8   | `u32` versión (1) |
    /// | 8..12  | `u32` count |
    /// | 12..16 | `u32` stride (12) |
    /// | 16..20 | `f32` ancho (`w`) |
    /// | 20..24 | `f32` alto (`h`) |
    /// | 24..28 | `u32` nodo raíz |
    /// | 28..32 | `f32` alto de la franja de título |
    /// | 32..36 | `f32` margen de carpeta |
    /// | 36..40 | `u32` métrica (0 = en disco, 1 = lógico) |
    /// | 40..   | `count * stride` `f32` (rects) y después `count` `u32` (ids) |
    ///
    /// `w` y `h` son las medidas para las que se calculó (normalmente `self.width`
    /// y `self.height`).
    pub fn to_bytes(&self, w: f32, h: f32) -> Vec<u8> {
        let n = self.count;
        let mut out = Vec::with_capacity(BYTES_HEADER + n * STRIDE * 4 + n * 4);
        out.extend_from_slice(&BYTES_MAGIC);
        out.extend_from_slice(&BYTES_VERSION.to_le_bytes());
        out.extend_from_slice(&(n as u32).to_le_bytes());
        out.extend_from_slice(&(STRIDE as u32).to_le_bytes());
        out.extend_from_slice(&w.to_le_bytes());
        out.extend_from_slice(&h.to_le_bytes());
        out.extend_from_slice(&self.root.to_le_bytes());
        out.extend_from_slice(&self.header.to_le_bytes());
        out.extend_from_slice(&self.padding.to_le_bytes());
        out.extend_from_slice(&self.metric.code().to_le_bytes());
        debug_assert_eq!(out.len(), BYTES_HEADER);
        for v in &self.rects[..n * STRIDE] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for v in &self.ids[..n] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    fn reset(&mut self, root: NodeId, w: f32, h: f32, opts: &LayoutOpts) {
        self.rects.clear();
        self.ids.clear();
        self.count = 0;
        self.width = w;
        self.height = h;
        self.root = root;
        self.metric = opts.metric;
        self.header = opts.header.max(0.0);
        self.padding = opts.dir_padding.max(0.0);
        self.min_area = 0.0;
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(&mut self, id: NodeId, r: &[f64; 4], s: &Surf, depth: u32, cat: u8, fl: u32, parent: i32) -> usize {
        let i = self.ids.len();
        let (gx0, gx1) = (s.ax + s.bx * r[0], s.ax + s.bx * r[2]);
        let (gy0, gy1) = (s.ay + s.by * r[1], s.ay + s.by * r[3]);
        self.rects.extend_from_slice(&[
            r[0] as f32,
            r[1] as f32,
            r[2] as f32,
            r[3] as f32,
            gx0 as f32,
            gx1 as f32,
            gy0 as f32,
            gy1 as f32,
            depth as f32,
            cat as f32,
            fl as f32,
            parent as f32,
        ]);
        self.ids.push(id);
        i
    }

    fn set(&mut self, i: usize, cat: u8, fl: u32) {
        self.rects[i * STRIDE + field::CAT] = cat as f32;
        self.rects[i * STRIDE + field::FLAGS] = fl as f32;
    }
}

/// Superficie acumulada: `dz/dx = ax + bx·x`, `dz/dy = ay + by·y`.
#[derive(Clone, Copy, Debug, Default)]
struct Surf {
    ax: f64,
    bx: f64,
    ay: f64,
    by: f64,
}

impl Surf {
    /// Suma la cresta `z = 4h(x-x0)(x1-x)/(x1-x0)` (y lo mismo en y).
    fn add_ridge(&mut self, r: &[f64; 4], h: f64) {
        let w = r[2] - r[0];
        if w > 1e-9 {
            let k = 4.0 * h / w;
            self.ax += k * (r[0] + r[2]);
            self.bx -= 2.0 * k;
        }
        let hh = r[3] - r[1];
        if hh > 1e-9 {
            let k = 4.0 * h / hh;
            self.ay += k * (r[1] + r[3]);
            self.by -= 2.0 * k;
        }
    }
}

const NODE: u8 = 0;
const REST: u8 = 1;

/// Rectángulo pendiente de emitir (pila del recorrido en preorden).
#[derive(Clone, Copy, Debug)]
struct Pending {
    node: NodeId,
    parent: i32,
    depth: u32,
    kind: u8,
    cat: u8,
    r: [f64; 4],
    /// Superficie del padre (la cresta propia se suma al emitir).
    s: Surf,
    /// Altura de la cresta propia.
    h: f64,
}

#[derive(Clone, Copy, Debug)]
struct Item {
    node: NodeId,
    v: u64,
}

#[derive(Clone, Debug, Default)]
struct Scratch {
    stack: Vec<Pending>,
    items: Vec<Item>,
    boxes: Vec<[f64; 4]>,
    ext_cats: Vec<u8>,
    /// Carpetas cuyos hijos pueden estar desordenados (antepasados de nodos borrados).
    dirty: Vec<u64>,
}

/// Calcula el treemap de `root` en un área de `w` × `h` px.
pub fn layout(tree: &Tree, root: NodeId, w: f32, h: f32, opts: &LayoutOpts) -> Layout {
    let mut l = Layout::default();
    layout_into(tree, root, w, h, opts, &mut l);
    l
}

/// Igual que [`layout`], pero reutiliza la memoria de `out` (búferes y auxiliares).
pub fn layout_into(tree: &Tree, root: NodeId, w: f32, h: f32, opts: &LayoutOpts, out: &mut Layout) {
    out.reset(root, w, h, opts);
    if (root as usize) >= tree.len() || !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
        return;
    }
    let vals = opts.metric.values(tree);
    if vals[root as usize] == 0 {
        return;
    }
    let mut sc = std::mem::take(&mut out.scratch);
    sc.stack.clear();
    sc.ext_cats.clear();
    sc.ext_cats.extend(tree.exts.iter().map(|e| categories::category(e)));
    mark_dirty(tree, &mut sc.dirty);

    let (wf, hf) = (w as f64, h as f64);
    let mut min_area = (opts.min_area as f64).max(0.01);
    if opts.max_rects > 0 {
        min_area = min_area.max(wf * hf / opts.max_rects as f64);
    }
    out.min_area = min_area as f32;

    let ctx = Ctx {
        tree,
        vals,
        opts,
        min_area,
        falloff: opts.cushion_falloff.clamp(0.0, 1.0) as f64,
        ext_cats: &sc.ext_cats,
        dirty: &sc.dirty,
    };
    let full = [0.0, 0.0, wf, hf];
    let h0 = opts.cushion_height.max(0.0) as f64;
    if tree.is_dir(root) {
        // la raíz no se dibuja: su contenido ocupa toda la vista
        let mut bufs = Bufs { stack: &mut sc.stack, items: &mut sc.items, boxes: &mut sc.boxes };
        ctx.expand(root, full, 1, -1, Surf::default(), h0, true, &mut bufs);
        while let Some(p) = bufs.stack.pop() {
            ctx.visit(p, &mut bufs, out);
        }
    } else {
        // la raíz es un archivo: un único rectángulo
        let mut s = Surf::default();
        s.add_ridge(&full, h0);
        let synth = if tree.flags[root as usize] & flags::SYNTHETIC != 0 { rflags::SYNTHETIC } else { 0 };
        out.emit(root, &full, &s, 1, ctx.file_cat(root), synth, -1);
    }
    out.count = out.ids.len();
    out.scratch = sc;
}

struct Bufs<'b> {
    stack: &'b mut Vec<Pending>,
    items: &'b mut Vec<Item>,
    boxes: &'b mut Vec<[f64; 4]>,
}

struct Ctx<'a> {
    tree: &'a Tree,
    vals: &'a [u64],
    opts: &'a LayoutOpts,
    min_area: f64,
    falloff: f64,
    ext_cats: &'a [u8],
    dirty: &'a [u64],
}

impl Ctx<'_> {
    fn visit(&self, p: Pending, b: &mut Bufs, out: &mut Layout) {
        let r = p.r;
        let (w, h) = (r[2] - r[0], r[3] - r[1]);
        if !(w > 0.0 && h > 0.0) {
            return;
        }
        let mut s = p.s;
        s.add_ridge(&r, p.h);
        if p.kind == REST {
            out.emit(p.node, &r, &s, p.depth, p.cat, rflags::REST, p.parent);
            return;
        }
        let n = p.node;
        let tf = self.tree.flags[n as usize];
        let synth = if tf & flags::SYNTHETIC != 0 { rflags::SYNTHETIC } else { 0 };
        if tf & flags::DIR == 0 {
            out.emit(n, &r, &s, p.depth, self.file_cat(n), synth, p.parent);
            return;
        }
        let idx = out.emit(n, &r, &s, p.depth, CAT_DIR, rflags::DIR | synth, p.parent);
        let (pad, head) = self.frame(w, h);
        let c = [r[0] + pad, r[1] + pad.max(head), r[2] - pad, r[3] - pad];
        let (cw, ch) = (c[2] - c[0], c[3] - c[1]);
        let room = cw >= MIN_SIDE && ch >= MIN_SIDE && cw * ch >= self.min_area;
        // un marco sin franja ni margen no se ve: si solo cabría un «resto», mejor maciza
        let framed = pad > 0.0 || head > 0.0;
        if room && p.depth < self.opts.max_depth && self.expand(n, c, p.depth + 1, idx as i32, s, p.h * self.falloff, framed, b) {
            let mut fl = rflags::DIR | synth;
            if pad > 0.0 {
                fl |= rflags::PADDED;
            }
            if head > 0.0 {
                fl |= rflags::HEADER;
            }
            out.set(idx, CAT_DIR, fl);
        } else {
            // sin desplegar: se pinta maciza con el color de lo que más ocupa dentro
            out.set(idx, self.dominant(n), rflags::DIR | rflags::COLLAPSED | synth);
        }
    }

    /// Margen y franja de título de una carpeta de `w` × `h` px.
    fn frame(&self, w: f64, h: f64) -> (f64, f64) {
        let p = self.opts.dir_padding.max(0.0) as f64;
        let pad = if p > 0.0 && w >= 8.0 * p + 4.0 && h >= 8.0 * p + 4.0 { p } else { 0.0 };
        let hd = self.opts.header.max(0.0) as f64;
        let head = if hd > 0.0 && w >= (4.0 * hd).max(48.0) && h >= 3.0 * hd + pad { hd } else { 0.0 };
        (pad, head)
    }

    /// Reparte los hijos de `dir` en `c` y los apila. Devuelve si hay algo que dibujar;
    /// con `rest_only` vale también un único «resto» que lo ocupe todo.
    #[allow(clippy::too_many_arguments)]
    fn expand(&self, dir: NodeId, c: [f64; 4], depth: u32, parent: i32, s: Surf, h: f64, rest_only: bool, b: &mut Bufs) -> bool {
        let total = self.vals[dir as usize];
        if total == 0 {
            return false;
        }
        let area = (c[2] - c[0]) * (c[3] - c[1]);
        // valor mínimo para dibujarse aparte
        let thr = self.min_area * total as f64 / area;
        let t = self.tree;
        let skip = flags::HARDLINK | flags::DELETED;
        b.items.clear();
        let mut rest_node = NONE;
        let mut rest_max = 0u64;
        // primero el valor y solo si hace falta los flags: cada lectura es un fallo de caché
        if self.opts.metric == Metric::Alloc && !self.is_dirty(dir) {
            // ordenados de mayor a menor: se para en el primero que no llega
            for &k in t.children(dir) {
                let v = self.vals[k as usize];
                if v == 0 {
                    break;
                }
                if t.flags[k as usize] & skip != 0 {
                    continue;
                }
                if (v as f64) < thr {
                    rest_node = k;
                    break;
                }
                prefetch(t, k);
                b.items.push(Item { node: k, v });
            }
        } else {
            for &k in t.children(dir) {
                let v = self.vals[k as usize];
                if v == 0 || ((v as f64) < thr && v <= rest_max) {
                    continue;
                }
                if t.flags[k as usize] & skip != 0 {
                    continue;
                }
                if (v as f64) >= thr {
                    prefetch(t, k);
                    b.items.push(Item { node: k, v });
                } else {
                    rest_max = v;
                    rest_node = k;
                }
            }
            // estable: a igual valor manda el orden del árbol
            b.items.sort_by(|x, y| y.v.cmp(&x.v));
        }
        if b.items.is_empty() && !rest_only {
            return false;
        }
        let sum: u64 = b.items.iter().map(|i| i.v).sum();
        let total = total.max(sum);
        b.boxes.clear();
        let rest = squarify(b.items, total, c, b.boxes);
        let rest_area = (rest[2] - rest[0]).max(0.0) * (rest[3] - rest[1]).max(0.0);
        let mut any = !b.items.is_empty();
        // el resto se apila primero para que salga el último de sus hermanos
        if total > sum && rest_area >= self.min_area {
            let cat = if rest_node != NONE { self.cat_of(rest_node) } else { SISTEMA };
            b.stack.push(Pending { node: dir, parent, depth, kind: REST, cat, r: rest, s, h });
            any = true;
        }
        for k in (0..b.items.len()).rev() {
            b.stack.push(Pending { node: b.items[k].node, parent, depth, kind: NODE, cat: 0, r: b.boxes[k], s, h });
        }
        any
    }

    fn is_dirty(&self, n: NodeId) -> bool {
        !self.dirty.is_empty() && self.dirty[n as usize >> 6] & (1u64 << (n & 63)) != 0
    }

    fn file_cat(&self, n: NodeId) -> u8 {
        categories::node_category(self.tree, self.ext_cats, n)
    }

    fn cat_of(&self, n: NodeId) -> u8 {
        if self.tree.is_dir(n) {
            self.dominant(n)
        } else {
            self.file_cat(n)
        }
    }

    /// Categoría aproximada de una carpeta: la del archivo al que se llega bajando
    /// siempre por el hijo más grande (pocos niveles: solo da color a lo diminuto).
    fn dominant(&self, mut n: NodeId) -> u8 {
        let t = self.tree;
        for _ in 0..8 {
            if !t.is_dir(n) {
                return self.file_cat(n);
            }
            let next = t.children(n).iter().copied().find(|&c| t.flags[c as usize] & (flags::HARDLINK | flags::DELETED) == 0 && self.vals[c as usize] > 0);
            match next {
                Some(c) => n = c,
                None => return OTROS,
            }
        }
        OTROS
    }
}

/// Pide a la caché los datos que `visit` leerá de un hijo recién apilado.
#[inline(always)]
fn prefetch(t: &Tree, k: NodeId) {
    #[cfg(target_arch = "x86_64")]
    // SAFETY: solo es una pista para la caché; los punteros están dentro de los vectores.
    unsafe {
        use std::arch::x86_64::{_mm_prefetch, _MM_HINT_T0};
        let k = k as usize;
        _mm_prefetch::<_MM_HINT_T0>(t.ext.as_ptr().add(k) as *const i8);
        _mm_prefetch::<_MM_HINT_T0>(t.child_start.as_ptr().add(k) as *const i8);
    }
    #[cfg(not(target_arch = "x86_64"))]
    let _ = (t, k);
}

/// Marca los antepasados de los nodos borrados (sus listas de hijos ya no están ordenadas).
fn mark_dirty(tree: &Tree, bits: &mut Vec<u64>) {
    bits.clear();
    if !tree.flags.iter().any(|&f| f & flags::DELETED != 0) {
        return;
    }
    bits.resize(tree.len().div_ceil(64), 0);
    for (i, &f) in tree.flags.iter().enumerate() {
        if f & flags::DELETED == 0 {
            continue;
        }
        let mut p = tree.parent[i];
        while p != NONE {
            let (w, m) = (p as usize >> 6, 1u64 << (p & 63));
            if bits[w] & m != 0 {
                break;
            }
            bits[w] |= m;
            p = tree.parent[p as usize];
        }
    }
}

/// Peor proporción de una fila de área `sum` (máximo `max`, mínimo `min`) sobre un lado² `s2`.
#[inline]
fn worst(sum: f64, max: f64, min: f64, s2: f64) -> f64 {
    let ss = sum * sum;
    (s2 * max / ss).max(ss / (s2 * min))
}

/// Algoritmo «squarified»: coloca `items` (de mayor a menor) en `c`, en proporción a
/// su valor sobre `total`. Escribe una caja por elemento en `boxes` y devuelve lo que
/// sobra (el hueco del «resto», con área proporcional a `total - Σ items`).
fn squarify(items: &[Item], total: u64, c: [f64; 4], boxes: &mut Vec<[f64; 4]>) -> [f64; 4] {
    let [mut x0, mut y0, x1, y1] = c;
    let n = items.len();
    if n == 0 || total == 0 {
        return c;
    }
    let scale = (x1 - x0) * (y1 - y0) / total as f64;
    let mut placed: u64 = 0;
    let mut i = 0;
    while i < n {
        let (w, h) = (x1 - x0, y1 - y0);
        if !(w > 0.0 && h > 0.0) {
            // sin sitio por redondeo: cajas vacías
            boxes.extend(std::iter::repeat_n([x0, y0, x0, y0], n - i));
            return [x0, y0, x0, y0];
        }
        // la fila va a lo largo del lado corto: columna a la izquierda si es apaisado
        let vertical = w >= h;
        let side = if vertical { h } else { w };
        let s2 = side * side;
        let a0 = items[i].v as f64 * scale;
        let (mut sum, mut row_v, mut j) = (a0, items[i].v, i + 1);
        let mut cur = worst(sum, a0, a0, s2);
        while j < n {
            let a = items[j].v as f64 * scale;
            let next = worst(sum + a, a0, a, s2);
            if next > cur {
                break;
            }
            sum += a;
            row_v += items[j].v;
            cur = next;
            j += 1;
        }
        placed += row_v;
        let last = j == n && placed >= total;
        let edge = match (vertical, last) {
            (true, true) => x1,
            (false, true) => y1,
            (true, false) => (x0 + sum / side).min(x1),
            (false, false) => (y0 + sum / side).min(y1),
        };
        let (start, end) = if vertical { (y0, y1) } else { (x0, x1) };
        let (mut pos, mut acc) = (start, 0.0);
        for (k, it) in items.iter().enumerate().take(j).skip(i) {
            acc += it.v as f64 * scale;
            let next = if k + 1 == j { end } else { (start + side * (acc / sum)).min(end) };
            boxes.push(if vertical { [x0, pos, edge, next] } else { [pos, y0, next, edge] });
            pos = next;
        }
        if vertical {
            x0 = edge;
        } else {
            y0 = edge;
        }
        i = j;
    }
    [x0, y0, x1, y1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Builder;

    const EPS: f32 = 1e-3;

    fn opts_flat() -> LayoutOpts {
        LayoutOpts { min_area: 0.01, dir_padding: 0.0, header: 0.0, max_rects: 0, ..Default::default() }
    }

    /// raíz con carpetas y archivos de tamaños variados
    fn sample(seed: u64, n_files: usize) -> Tree {
        let mut rng = Rng(seed);
        let mut b = Builder::default();
        b.add(1, 0, "", 0, 0, 0, flags::DIR);
        let mut dirs = vec![1u64];
        let mut key = 2u64;
        for _ in 0..n_files / 8 + 1 {
            let p = dirs[rng.below(dirs.len() as u64) as usize];
            b.add(key, p, &format!("d{key}"), 0, 0, 0, flags::DIR);
            dirs.push(key);
            key += 1;
        }
        let exts = ["mp4", "jpg", "dll", "txt", "pak", "zip", "bin"];
        for _ in 0..n_files {
            let p = dirs[rng.below(dirs.len() as u64) as usize];
            let size = rng.lognormal(11.0, 2.0);
            let alloc = size.div_ceil(4096) * 4096;
            let e = exts[rng.below(exts.len() as u64) as usize];
            b.add(key, p, &format!("f{key}.{e}"), size, alloc, 0, 0);
            key += 1;
        }
        b.build(1, "C:\\")
    }

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            // splitmix64
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n.max(1)
        }
        fn unit(&mut self) -> f64 {
            (self.next() >> 11) as f64 / (1u64 << 53) as f64
        }
        fn lognormal(&mut self, mu: f64, sigma: f64) -> u64 {
            let (u1, u2) = (self.unit().max(1e-12), self.unit());
            let z = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
            (mu + sigma * z).exp().min(1e12) as u64 + 1
        }
    }

    fn area(l: &Layout, i: usize) -> f64 {
        let [_, _, w, h] = l.rect(i);
        w as f64 * h as f64
    }

    fn inside(inner: [f32; 4], outer: [f32; 4]) -> bool {
        inner[0] >= outer[0] - EPS && inner[1] >= outer[1] - EPS && inner[0] + inner[2] <= outer[0] + outer[2] + EPS && inner[1] + inner[3] <= outer[1] + outer[3] + EPS
    }

    fn overlap(a: [f32; 4], b: [f32; 4]) -> f64 {
        let w = ((a[0] + a[2]).min(b[0] + b[2]) - a[0].max(b[0])).max(0.0);
        let h = ((a[1] + a[3]).min(b[1] + b[3]) - a[1].max(b[1])).max(0.0);
        w as f64 * h as f64
    }

    /// Comprobaciones generales: límites, hijos dentro del padre, hermanos sin solaparse.
    fn check_geometry(l: &Layout, w: f32, h: f32) {
        assert_eq!(l.rects.len(), l.count * STRIDE);
        assert_eq!(l.ids.len(), l.count);
        let bounds = [0.0, 0.0, w, h];
        let mut siblings: std::collections::HashMap<i64, Vec<usize>> = Default::default();
        for i in 0..l.count {
            let r = l.rect(i);
            assert!(r.iter().all(|v| v.is_finite()) && r[2] > 0.0 && r[3] > 0.0, "rect {i}: {r:?}");
            assert!(inside(r, bounds), "rect {i} fuera: {r:?}");
            assert!(l.record(i)[4..8].iter().all(|v| v.is_finite()));
            match l.parent(i) {
                Some(p) => {
                    assert!(p < i, "el padre va antes");
                    assert!(l.flags(p) & rflags::DIR != 0);
                    assert!(inside(r, l.rect(p)), "rect {i} fuera de su padre {p}");
                    assert_eq!(l.depth(i), l.depth(p) + 1);
                    siblings.entry(p as i64).or_default().push(i);
                }
                None => {
                    assert_eq!(l.depth(i), 1);
                    siblings.entry(-1).or_default().push(i);
                }
            }
        }
        for group in siblings.values() {
            let group = &group[..group.len().min(1500)];
            for (a, &i) in group.iter().enumerate() {
                for &j in &group[a + 1..] {
                    let o = overlap(l.rect(i), l.rect(j));
                    assert!(o < 1e-3, "{i} y {j} se solapan {o}");
                }
            }
        }
    }

    #[test]
    fn areas_are_proportional() {
        let t = sample(7, 3000);
        let (w, h) = (1200.0, 800.0);
        let l = layout(&t, t.root, w, h, &opts_flat());
        check_geometry(&l, w, h);
        let total = t.alloc[t.root as usize] as f64;
        let scale = (w * h) as f64 / total;
        let mut checked = 0;
        for i in 0..l.count {
            if l.flags(i) & rflags::REST != 0 {
                continue;
            }
            let want = t.alloc[l.ids[i] as usize] as f64 * scale;
            let got = area(&l, i);
            if want > 50.0 {
                assert!((got - want).abs() / want < 2e-3, "{i}: {got} vs {want}");
                checked += 1;
            }
        }
        assert!(checked > 100, "{checked}");
        // los hijos de la raíz cubren toda la vista
        let covered: f64 = (0..l.count).filter(|&i| l.parent(i).is_none()).map(|i| area(&l, i)).sum();
        assert!((covered - (w * h) as f64).abs() < 1.0, "{covered}");
    }

    #[test]
    fn siblings_are_proportional_with_padding_and_headers() {
        let t = sample(11, 4000);
        let (w, h) = (1600.0, 900.0);
        let o = LayoutOpts { dir_padding: 3.0, header: 18.0, ..Default::default() };
        let l = layout(&t, t.root, w, h, &o);
        check_geometry(&l, w, h);
        assert!((0..l.count).any(|i| l.flags(i) & rflags::HEADER != 0));
        assert_eq!(l.header, 18.0);
        assert_eq!(l.padding, 3.0);
        // dentro de una carpeta, el área de cada hijo respecto al primero sigue a los tamaños
        let mut groups: std::collections::HashMap<usize, Vec<usize>> = Default::default();
        for i in 0..l.count {
            if let Some(p) = l.parent(i) {
                if l.flags(i) & rflags::REST == 0 {
                    groups.entry(p).or_default().push(i);
                }
            }
        }
        let mut checked = 0;
        for (p, kids) in groups {
            let f = l.flags(p);
            let r = l.rect(p);
            let pad = if f & rflags::PADDED != 0 { 3.0 } else { 0.0 };
            let top = if f & rflags::HEADER != 0 { 18.0f32.max(pad) } else { pad };
            for &k in &kids {
                assert!(l.rect(k)[1] >= r[1] + top - EPS, "el contenido empieza bajo la franja");
            }
            let a0 = area(&l, kids[0]);
            let v0 = t.alloc[l.ids[kids[0]] as usize] as f64;
            for &k in &kids[1..] {
                let ratio_area = area(&l, k) / a0;
                let ratio_val = t.alloc[l.ids[k] as usize] as f64 / v0;
                if area(&l, k) > 30.0 {
                    assert!((ratio_area - ratio_val).abs() / ratio_val < 5e-3, "{ratio_area} vs {ratio_val}");
                    checked += 1;
                }
            }
        }
        assert!(checked > 50);
    }

    #[test]
    fn deterministic_and_reusable() {
        let t = sample(3, 5000);
        let o = LayoutOpts::default();
        let a = layout(&t, t.root, 1920.0, 1080.0, &o);
        let b = layout(&t, t.root, 1920.0, 1080.0, &o);
        let bits = |l: &Layout| l.rects.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
        assert_eq!(bits(&a), bits(&b));
        assert_eq!(a.ids, b.ids);
        // reutilizando el mismo Layout (con otro tamaño entre medias) sale igual
        let mut c = Layout::default();
        layout_into(&t, t.root, 300.0, 200.0, &o, &mut c);
        layout_into(&t, t.root, 1920.0, 1080.0, &o, &mut c);
        assert_eq!(bits(&a), bits(&c));
        assert_eq!(a.ids, c.ids);
    }

    #[test]
    fn neighbours_share_exact_edges() {
        // sin márgenes los hijos de la raíz cubren la vista: el vecino de la derecha de
        // cada uno empieza exactamente (mismo f32) donde acaba él
        let t = sample(5, 400);
        let w = 1000.0;
        let l = layout(&t, t.root, w, 700.0, &opts_flat());
        let tops: Vec<usize> = (0..l.count).filter(|&i| l.parent(i).is_none()).collect();
        for &i in &tops {
            let a = l.record(i);
            if a[field::X1] >= w {
                continue;
            }
            let found = tops.iter().any(|&j| {
                let b = l.record(j);
                b[field::X0].to_bits() == a[field::X1].to_bits() && a[field::Y0] < b[field::Y1] && b[field::Y0] < a[field::Y1]
            });
            assert!(found, "sin vecino exacto a la derecha de {i}: {a:?}");
        }
    }

    #[test]
    fn empty_and_degenerate_inputs() {
        let t = sample(1, 100);
        let o = LayoutOpts::default();
        assert!(layout(&t, t.root, 0.0, 100.0, &o).is_empty());
        assert!(layout(&t, t.root, f32::NAN, 100.0, &o).is_empty());
        assert!(layout(&t, t.root, 100.0, -5.0, &o).is_empty());
        assert!(layout(&t, t.root, f32::INFINITY, 100.0, &o).is_empty());
        assert!(layout(&t, 999_999, 100.0, 100.0, &o).is_empty());

        // todo a cero
        let mut b = Builder::default();
        b.add(1, 0, "", 0, 0, 0, flags::DIR);
        b.add(2, 1, "vacio.txt", 0, 0, 0, 0);
        b.add(3, 1, "dentro", 0, 0, 0, flags::DIR);
        let z = b.build(1, "Z:\\");
        assert!(layout(&z, z.root, 800.0, 600.0, &o).is_empty());

        // un archivo enorme y unos pocos diminutos
        let mut b = Builder::default();
        b.add(1, 0, "", 0, 0, 0, flags::DIR);
        b.add(2, 1, "enorme.vhdx", 1 << 40, 1 << 40, 0, 0);
        for k in 0..50 {
            b.add(10 + k, 1, &format!("p{k}.txt"), 10, 4096, 0, 0);
        }
        let t = b.build(1, "C:\\");
        let l = layout(&t, t.root, 1000.0, 600.0, &o);
        check_geometry(&l, 1000.0, 600.0);
        assert_eq!(l.count, 1, "los diminutos no llegan ni a resto");
        assert_eq!(t.name(l.ids[0]), "enorme.vhdx");
        assert_eq!(l.category(0), categories::COMPRIMIDO);
        assert!(area(&l, 0) > 1000.0 * 600.0 * 0.999);

        // la raíz es un archivo
        let f = l.ids[0];
        let one = layout(&t, f, 640.0, 480.0, &o);
        assert_eq!(one.count, 1);
        assert_eq!(one.rect(0), [0.0, 0.0, 640.0, 480.0]);
    }

    #[test]
    fn thousands_of_tiny_files() {
        let mut b = Builder::default();
        b.add(1, 0, "", 0, 0, 0, flags::DIR);
        b.add(2, 1, "node_modules", 0, 0, 0, flags::DIR);
        for k in 0..5000u64 {
            b.add(10 + k, 2, &format!("m{k}.js"), 900, 4096, 0, 0);
        }
        let t = b.build(1, "C:\\");
        let o = LayoutOpts::default();
        // en 100×100 cada uno tocaría a 2 px²: todo va a un resto dentro de la carpeta
        let l = layout(&t, t.root, 100.0, 100.0, &o);
        check_geometry(&l, 100.0, 100.0);
        assert_eq!(l.count, 2);
        assert!(l.flags(0) & rflags::DIR != 0 && l.flags(0) & rflags::COLLAPSED == 0);
        assert!(l.flags(1) & rflags::REST != 0);
        assert_eq!(l.ids[1], l.ids[0]);
        assert_eq!(l.category(1), categories::CODIGO);
        // con sitio de sobra salen todos
        let big = layout(&t, t.root, 1000.0, 1000.0, &o);
        check_geometry(&big, 1000.0, 1000.0);
        assert_eq!(big.count, 5001);
        // y en 10×10 la carpeta queda maciza
        let tiny = layout(&t, t.root, 10.0, 10.0, &o);
        assert_eq!(tiny.count, 1);
        assert!(tiny.flags(0) & rflags::COLLAPSED != 0);
        assert_eq!(tiny.category(0), categories::CODIGO);
    }

    #[test]
    fn rest_aggregates_small_children() {
        let mut b = Builder::default();
        b.add(1, 0, "", 0, 0, 0, flags::DIR);
        b.add(2, 1, "peli.mkv", 1 << 30, 1 << 30, 0, 0);
        for k in 0..20000u64 {
            b.add(10 + k, 1, &format!("c{k}.log"), 30_000, 32_768, 0, 0);
        }
        let t = b.build(1, "C:\\");
        let l = layout(&t, t.root, 400.0, 300.0, &LayoutOpts::default());
        check_geometry(&l, 400.0, 300.0);
        let rest: Vec<usize> = (0..l.count).filter(|&i| l.flags(i) & rflags::REST != 0).collect();
        assert_eq!(rest.len(), 1);
        let want = 20000.0 * 32768.0 / t.alloc[t.root as usize] as f64 * 400.0 * 300.0;
        assert!((area(&l, rest[0]) - want).abs() / want < 1e-3);
        assert_eq!(l.category(rest[0]), SISTEMA);
        assert_eq!(l.ids[rest[0]], t.root);
    }

    #[test]
    fn hardlinks_and_deleted_are_skipped() {
        let mut b = Builder::default();
        b.add(1, 0, "", 0, 0, 0, flags::DIR);
        b.add(2, 1, "a", 0, 0, 0, flags::DIR);
        b.add(3, 2, "grande.mp4", 9000 << 20, 9000 << 20, 0, 0);
        b.add(8, 2, "nota.txt", 100, 4096, 0, 0);
        b.add(4, 1, "b", 0, 0, 0, flags::DIR);
        b.add(3, 4, "enlace.mp4", 9000 << 20, 9000 << 20, 0, 0);
        b.add(5, 4, "mediano.zip", 3000 << 20, 3000 << 20, 0, 0);
        b.add(6, 1, "c.iso", 4000 << 20, 4000 << 20, 0, 0);
        b.add(7, 1, "d.iso", 1000 << 20, 1000 << 20, 0, 0);
        let mut t = b.build(1, "C:\\");
        let by_name = |t: &Tree, name: &str| (0..t.len() as u32).find(|&n| t.name(n) == name).unwrap();
        let link = by_name(&t, "enlace.mp4");
        assert!(t.flags[link as usize] & flags::HARDLINK != 0);
        let l = layout(&t, t.root, 800.0, 500.0, &opts_flat());
        check_geometry(&l, 800.0, 500.0);
        assert!(!l.ids.contains(&link));
        assert_eq!(t.name(t.children(t.root)[0]), "a");

        // borrar «grande.mp4» como hace la aplicación: queda a cero y marcado, sus
        // antepasados encogen y la lista de la raíz deja de estar ordenada («a» queda
        // la primera con 4 KB). Lo que viene detrás tiene que seguir saliendo.
        let g = by_name(&t, "grande.mp4");
        let freed = t.alloc[g as usize];
        t.alloc[g as usize] = 0;
        t.size[g as usize] = 0;
        t.flags[g as usize] |= flags::DELETED;
        let mut p = t.parent[g as usize];
        while p != NONE {
            t.alloc[p as usize] -= freed;
            t.size[p as usize] -= freed;
            p = t.parent[p as usize];
        }
        let l = layout(&t, t.root, 800.0, 500.0, &LayoutOpts::default());
        check_geometry(&l, 800.0, 500.0);
        let names: Vec<&str> = l.ids.iter().map(|&n| t.name(n)).collect();
        assert!(!names.contains(&"grande.mp4"));
        for want in ["b", "mediano.zip", "c.iso", "d.iso"] {
            assert!(names.contains(&want), "falta {want}: {names:?}");
        }
    }

    #[test]
    fn max_depth_collapses() {
        let t = sample(9, 3000);
        let o = LayoutOpts { max_depth: 2, ..Default::default() };
        let l = layout(&t, t.root, 1200.0, 800.0, &o);
        check_geometry(&l, 1200.0, 800.0);
        assert!((0..l.count).all(|i| l.depth(i) <= 2));
        for i in 0..l.count {
            if l.depth(i) == 2 && l.flags(i) & rflags::DIR != 0 {
                assert!(l.flags(i) & rflags::COLLAPSED != 0);
                assert!((l.category(i) as usize) < categories::COUNT);
            }
        }
        let one = layout(&t, t.root, 1200.0, 800.0, &LayoutOpts { max_depth: 1, ..Default::default() });
        assert!((0..one.count).all(|i| one.depth(i) == 1));
    }

    #[test]
    fn size_metric_orders_by_logical_size() {
        let mut b = Builder::default();
        b.add(1, 0, "", 0, 0, 0, flags::DIR);
        // comprimido en disco: mucho tamaño lógico y poco asignado
        b.add(2, 1, "comprimido.log", 8000 << 20, 100 << 20, 0, flags::COMPRESSED);
        b.add(3, 1, "normal.bin", 2000 << 20, 2000 << 20, 0, 0);
        let t = b.build(1, "C:\\");
        let first = |m: Metric| {
            let l = layout(&t, t.root, 500.0, 500.0, &LayoutOpts { metric: m, ..opts_flat() });
            check_geometry(&l, 500.0, 500.0);
            (0..l.count).max_by(|&i, &j| area(&l, i).total_cmp(&area(&l, j))).map(|i| t.name(l.ids[i]).to_string()).unwrap()
        };
        assert_eq!(first(Metric::Alloc), "normal.bin");
        assert_eq!(first(Metric::Size), "comprimido.log");
        let l = layout(&t, t.root, 500.0, 500.0, &LayoutOpts { metric: Metric::Size, ..opts_flat() });
        let i = (0..l.count).find(|&i| t.name(l.ids[i]) == "comprimido.log").unwrap();
        assert_eq!(l.find(l.ids[i]), Some(i));
        assert!((area(&l, i) / 250_000.0 - 0.8).abs() < 1e-3);
        assert_eq!(Metric::parse("size"), Some(Metric::Size));
        assert_eq!(Metric::parse("x"), None);
    }

    #[test]
    fn cushions_are_lit_from_the_ridges() {
        let t = sample(2, 500);
        let l = layout(&t, t.root, 900.0, 600.0, &LayoutOpts::default());
        for i in 0..l.count {
            let r = l.record(i);
            // la cresta propia sube por la izquierda/arriba y baja por la derecha/abajo
            assert!(r[field::GX0] > r[field::GX1], "{i}: {r:?}");
            assert!(r[field::GY0] > r[field::GY1], "{i}: {r:?}");
        }
    }

    #[test]
    fn max_rects_raises_min_area() {
        let t = sample(4, 20000);
        let l = layout(&t, t.root, 1000.0, 1000.0, &LayoutOpts { min_area: 0.01, max_rects: 2000, ..Default::default() });
        assert!(l.min_area >= 500.0 - 1e-3);
        let leaves = (0..l.count).filter(|&i| l.flags(i) & rflags::DIR == 0 || l.flags(i) & rflags::COLLAPSED != 0).count();
        assert!(leaves <= 2000, "{leaves}");
    }

    #[test]
    fn bytes_round_trip() {
        let t = sample(8, 300);
        let l = layout(&t, t.root, 640.0, 360.0, &LayoutOpts::default());
        let b = l.to_bytes(640.0, 360.0);
        assert_eq!(b.len(), BYTES_HEADER + l.count * (STRIDE * 4 + 4));
        assert_eq!(&b[0..4], b"DSVL");
        let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let f32_at = |o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        assert_eq!(u32_at(4), BYTES_VERSION);
        assert_eq!(u32_at(8) as usize, l.count);
        assert_eq!(u32_at(12) as usize, STRIDE);
        assert_eq!((f32_at(16), f32_at(20)), (640.0, 360.0));
        assert_eq!(u32_at(24), t.root);
        assert_eq!((f32_at(28), f32_at(32)), (l.header, l.padding));
        assert_eq!(u32_at(36), 0);
        let last = l.count - 1;
        assert_eq!(f32_at(BYTES_HEADER + (last * STRIDE + field::X1) * 4), l.record(last)[field::X1]);
        assert_eq!(u32_at(BYTES_HEADER + l.count * STRIDE * 4 + last * 4), l.ids[last]);
        let cands = l.label_candidates(40.0, 16.0);
        assert!(cands.iter().all(|&i| (i as usize) < l.count));
    }

    /// Árbol de ~2M nodos parecido a un volumen entero.
    fn volume(n_nodes: usize) -> Tree {
        let mut rng = Rng(42);
        let mut b = Builder::with_capacity(n_nodes);
        b.add(1, 0, "", 0, 0, 0, flags::DIR);
        let n_dirs = n_nodes / 10;
        for d in 0..n_dirs as u64 {
            let p = if d < 24 { 1 } else { 2 + rng.below(d) };
            b.add(2 + d, p, &format!("carpeta{d}"), 0, 0, 0, flags::DIR);
        }
        let exts = ["dll", "js", "json", "png", "mp4", "txt", "pak", "uasset", "h", "cpp", "log", "jpg", "pdf", "exe"];
        let base = 2 + n_dirs as u64;
        for f in 0..(n_nodes - n_dirs - 1) as u64 {
            // muchas carpetas con pocos archivos y unas pocas con miles
            let u = rng.unit();
            let p = 2 + ((u * u * u) * n_dirs as f64) as u64;
            let size = rng.lognormal(10.0, 2.3);
            let alloc = if size < 700 { 0 } else { size.div_ceil(4096) * 4096 };
            let e = exts[rng.below(exts.len() as u64) as usize];
            b.add(base + f, p, &format!("archivo{f}.{e}"), size, alloc, 0, 0);
        }
        b.build(1, "C:\\")
    }

    /// `cargo test --release -p desvan-core -- --ignored --nocapture layout_bench`
    #[test]
    #[ignore]
    fn layout_bench() {
        let t0 = std::time::Instant::now();
        let t = volume(2_000_000);
        println!("árbol: {} nodos en {:?}", t.len(), t0.elapsed());
        for (name, o) in [
            ("en disco", LayoutOpts::default()),
            ("lógico", LayoutOpts { metric: Metric::Size, ..Default::default() }),
            ("sin franjas", LayoutOpts { header: 0.0, dir_padding: 0.0, ..Default::default() }),
        ] {
            let mut l = Layout::default();
            let mut times = Vec::new();
            for _ in 0..7 {
                let s = std::time::Instant::now();
                layout_into(&t, t.root, 1920.0, 1080.0, &o, &mut l);
                times.push(s.elapsed().as_secs_f64() * 1000.0);
            }
            times.sort_by(f64::total_cmp);
            let dirs = (0..l.count).filter(|&i| l.flags(i) & rflags::DIR != 0).count();
            println!("{name}: {} rects ({dirs} carpetas): mín {:.2} ms, mediana {:.2} ms, máx {:.2} ms", l.count, times[0], times[3], times[6]);
            if !cfg!(debug_assertions) {
                assert!(times[3] < 50.0, "{name}: {:.2} ms", times[3]);
            }
        }
    }
}
