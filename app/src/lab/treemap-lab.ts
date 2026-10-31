// Laboratorio visual del treemap: carga los layouts que vuelca
// `cargo run --release -p desvan-core --example layout_dump -- app/src/lab/data/lab.bin 1280 840`
// y los muestra con TreemapView, con zoom, selección, resaltado por categoría y tooltip.

import "./treemap-lab.css";
import { CATEGORIES, CATEGORY_COLORS, FIELD, RFLAG, STRIDE, TreemapView, decodeLayout, isFrame, type DecodedLayout, type TreemapHit } from "../lib/treemap";

interface ViewMeta {
  id: number;
  parent: number | null;
  file: string;
  name: string;
  path: string;
}

interface Manifest {
  root: number;
  rootPath: string;
  width: number;
  height: number;
  volume: { total: number; used: number; label: string; filesystem: string };
  nodes: number;
  views: ViewMeta[];
}

interface Side {
  view: number;
  parentView: number | null;
  path: string;
  name: string;
  total: number;
  files: number;
  dirs: number;
  count: number;
  layoutMs: number;
  catTotals: number[];
  names: string[];
  sizes: number[];
  counts: number[];
}

interface Loaded {
  layout: DecodedLayout;
  side: Side;
  meta: ViewMeta;
}

const DATA = "/src/lab/data/";
const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const nf = new Intl.NumberFormat("es-ES");

/** "1,74 TB" (base 1024, coma decimal, como util::human en Rust). */
function bytes(n: number): string {
  const u = ["B", "KB", "MB", "GB", "TB", "PB"];
  if (n < 1024) return `${n} B`;
  let v = n;
  let i = 0;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  const s = v >= 100 ? v.toFixed(0) : v >= 10 ? v.toFixed(1) : v.toFixed(2);
  return `${s.replace(".", ",")} ${u[i]}`;
}

function pct(part: number, whole: number): string {
  if (!whole) return "";
  const p = (part / whole) * 100;
  return `${p >= 10 ? p.toFixed(0) : p >= 1 ? p.toFixed(1).replace(".", ",") : "<1"} %`;
}

let manifest: Manifest;
let cur: Loaded;
let view: TreemapView;
let sticky: number | null = null;
let selectedIndex = -1;
const cache = new Map<number, Promise<Loaded>>();

function load(id: number): Promise<Loaded> {
  let p = cache.get(id);
  if (!p) {
    const meta = manifest.views.find((v) => v.id === id);
    if (!meta) return Promise.reject(new Error(`sin vista para ${id}`));
    p = Promise.all([
      fetch(`${DATA}${meta.file}.bin`).then((r) => r.arrayBuffer()),
      fetch(`${DATA}${meta.file}.json`).then((r) => r.json() as Promise<Side>),
    ]).then(([bin, side]) => ({ layout: decodeLayout(bin), side, meta }));
    cache.set(id, p);
  }
  return p;
}

const field = (i: number, f: number) => cur.layout.rects[i * STRIDE + f];
const flagsOf = (i: number) => field(i, FIELD.FLAGS);

/** Ruta de un rectángulo: la de la vista más los nombres de sus antepasados. */
function pathOf(i: number): string {
  const parts: string[] = [];
  for (let k = i; k >= 0; k = field(k, FIELD.PARENT)) {
    if ((flagsOf(k) & RFLAG.REST) === 0) parts.push(cur.side.names[k]);
  }
  const base = cur.side.path.endsWith("\\") ? cur.side.path : cur.side.path + "\\";
  return base + parts.reverse().join("\\");
}

function kindOf(i: number): { label: string; color: string } {
  const f = flagsOf(i);
  const cat = field(i, FIELD.CAT);
  if (f & RFLAG.REST) return { label: "Agrupado: demasiado pequeños para verse sueltos", color: "#5d6575" };
  if (f & RFLAG.DIR) {
    const files = cur.side.counts[i];
    return { label: `Carpeta · ${nf.format(files)} archivos`, color: "#c9973f" };
  }
  return { label: CATEGORIES[cat]?.label ?? "Otros", color: CATEGORY_COLORS[cat] ?? CATEGORY_COLORS[0] };
}

// ---------- tooltip ----------

const tip = $("tip");
function showTip(hit: TreemapHit | null, ev: MouseEvent | null) {
  if (!hit || !ev) {
    tip.classList.remove("on");
    return;
  }
  const i = hit.index;
  const k = kindOf(i);
  const size = cur.side.sizes[i];
  tip.innerHTML = `
    <div class="t-name"></div>
    <div class="t-row"><span class="t-size">${bytes(size)}</span><span class="t-pct">${pct(size, cur.side.total)} de ${escapeHtml(cur.side.name)}</span></div>
    <div class="chip"><i style="background:${k.color}"></i><span></span></div>
    <div class="t-path"></div>`;
  (tip.querySelector(".t-name") as HTMLElement).textContent = cur.side.names[i];
  (tip.querySelector(".chip span") as HTMLElement).textContent = k.label;
  (tip.querySelector(".t-path") as HTMLElement).textContent = pathOf(i);
  placeTip(ev.offsetX, ev.offsetY);
  tip.classList.add("on");
}

function placeTip(x: number, y: number) {
  const stage = $("stage");
  const w = tip.offsetWidth || 260;
  const h = tip.offsetHeight || 110;
  let left = x + 16;
  let top = y + 18;
  if (left + w > stage.clientWidth - 8) left = x - w - 12;
  if (top + h > stage.clientHeight - 8) top = y - h - 12;
  tip.style.left = `${Math.max(8, left)}px`;
  tip.style.top = `${Math.max(8, top)}px`;
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
}

// ---------- panel lateral ----------

function renderSelection() {
  const el = $("sel");
  const i = selectedIndex;
  if (i < 0) {
    el.innerHTML = `<h2>Selección</h2>
      <div class="s-name"></div><div class="s-path"></div>
      <dl><dt>En disco</dt><dd>${bytes(cur.side.total)}</dd>
      <dt>Archivos</dt><dd>${nf.format(cur.side.files)}</dd>
      <dt>Carpetas</dt><dd>${nf.format(cur.side.dirs)}</dd>
      <dt>Rectángulos</dt><dd>${nf.format(cur.side.count)} · ${String(cur.side.layoutMs).replace(".", ",")} ms</dd></dl>`;
    (el.querySelector(".s-name") as HTMLElement).textContent = cur.side.name;
    (el.querySelector(".s-path") as HTMLElement).textContent = cur.side.path;
    return;
  }
  const k = kindOf(i);
  const size = cur.side.sizes[i];
  const isDir = (flagsOf(i) & RFLAG.DIR) !== 0;
  el.innerHTML = `<h2>Selección</h2>
    <div class="s-name"></div><div class="s-path"></div>
    <div class="chip"><i style="background:${k.color}"></i><span></span></div>
    <dl><dt>En disco</dt><dd>${bytes(size)}</dd>
    <dt>De ${escapeHtml(cur.side.name)}</dt><dd>${pct(size, cur.side.total)}</dd>
    <dt>Del volumen</dt><dd>${pct(size, manifest.volume.total)}</dd>
    ${isDir ? `<dt>Archivos</dt><dd>${nf.format(cur.side.counts[i])}</dd>` : ""}</dl>`;
  (el.querySelector(".s-name") as HTMLElement).textContent = cur.side.names[i];
  (el.querySelector(".s-path") as HTMLElement).textContent = pathOf(i);
  (el.querySelector(".chip span") as HTMLElement).textContent = k.label;
}

function renderLegend() {
  const ul = $("legend");
  const tot = cur.side.catTotals;
  const sum = tot.reduce((a, b) => a + b, 0) || 1;
  const order = CATEGORIES.map((c) => c.code).sort((a, b) => tot[b] - tot[a]);
  ul.innerHTML = "";
  for (const c of order) {
    const li = document.createElement("li");
    li.dataset.cat = String(c);
    li.innerHTML = `<span class="sw" style="background:${CATEGORY_COLORS[c]}"></span>
      <span class="lb"></span><span class="sz">${tot[c] ? bytes(tot[c]) : "—"}</span>
      <span class="bar"><i style="width:${((tot[c] / sum) * 100).toFixed(2)}%;background:${CATEGORY_COLORS[c]}"></i></span>`;
    (li.querySelector(".lb") as HTMLElement).textContent = CATEGORIES[c].label;
    li.addEventListener("mouseenter", () => highlight(c));
    li.addEventListener("mouseleave", () => highlight(sticky));
    li.addEventListener("click", () => {
      sticky = sticky === c ? null : c;
      highlight(sticky);
    });
    ul.appendChild(li);
  }
  markLegend(sticky);
}

function markLegend(c: number | null) {
  for (const li of Array.from($("legend").children) as HTMLElement[]) {
    const k = Number(li.dataset.cat);
    li.classList.toggle("on", c === k);
    li.classList.toggle("off", c !== null && c !== k);
  }
}

function highlight(c: number | null) {
  markLegend(c);
  if (c === null) {
    view.setHighlight(null);
    return;
  }
  // los marcos de carpeta no se apagan; el resto según su categoría
  view.setHighlight((i) => isFrame(flagsOf(i)) || field(i, FIELD.CAT) === c);
}

function renderCrumbs() {
  const nav = $("crumbs");
  nav.innerHTML = "";
  const chain: ViewMeta[] = [];
  for (let v: ViewMeta | undefined = cur.meta; v; v = manifest.views.find((x) => x.id === v!.parent)) chain.unshift(v);
  chain.forEach((v, k) => {
    if (k > 0) {
      const s = document.createElement("span");
      s.className = "sep";
      s.textContent = "›";
      nav.appendChild(s);
    }
    const b = document.createElement("button");
    b.textContent = v.id === manifest.root ? manifest.rootPath : v.name;
    if (v === cur.meta) b.className = "here";
    else b.addEventListener("click", () => goUp(v.id));
    nav.appendChild(b);
  });
}

function renderUsage() {
  const v = manifest.volume;
  $("usage").innerHTML = `<span><b>${bytes(v.used)}</b> de ${bytes(v.total)}</span>
    <span class="meter"><i style="width:${((v.used / v.total) * 100).toFixed(1)}%"></i></span>
    <span>${nf.format(manifest.nodes)} elementos</span>`;
}

// ---------- navegación ----------

function show(l: Loaded) {
  cur = l;
  selectedIndex = -1;
  view.setSelected(null);
  view.setData(l.layout.rects, l.layout.ids, l.layout.meta);
  if (sticky !== null) highlight(sticky);
  renderCrumbs();
  renderLegend();
  renderSelection();
}

/** Entra en la carpeta de primer nivel que contiene el rectángulo i (si hay vista). */
function zoomInto(i: number): boolean {
  let top = i;
  while (field(top, FIELD.PARENT) >= 0) top = field(top, FIELD.PARENT);
  // la más profunda de la cadena que tenga vista
  for (let k = i; k >= 0; k = field(k, FIELD.PARENT)) {
    const id = cur.layout.ids[k];
    if ((flagsOf(k) & RFLAG.DIR) !== 0 && (flagsOf(k) & RFLAG.REST) === 0 && manifest.views.some((v) => v.id === id && v.parent === cur.meta.id)) {
      top = k;
      break;
    }
  }
  const id = cur.layout.ids[top];
  if (!manifest.views.some((v) => v.id === id && v.parent === cur.meta.id)) return false;
  const r = view.rectAt(top);
  const next = load(id);
  tip.classList.remove("on");
  view.zoomTo([r.x, r.y, r.w, r.h], () => void next.then(show));
  return true;
}

function goUp(target?: number) {
  const up = target ?? cur.meta.parent;
  if (up === null || up === undefined) return;
  tip.classList.remove("on");
  view.zoomTo(null, () => void load(up).then(show));
}

// ---------- arranque ----------

async function main() {
  manifest = await (await fetch(`${DATA}lab.views.json`)).json();
  const canvas = $<HTMLCanvasElement>("map");
  canvas.style.width = `${manifest.width}px`;
  canvas.style.height = `${manifest.height}px`;
  view = new TreemapView(canvas, {
    labelFor: (i) => {
      const name = cur.side.names[i];
      return name ? { name, detail: bytes(cur.side.sizes[i]) } : null;
    },
    onHover: (hit, ev) => showTip(hit, ev),
    onClick: (hit) => {
      selectedIndex = hit ? hit.index : -1;
      view.setSelected(hit && (flagsOf(hit.index) & RFLAG.REST) === 0 ? hit.id : null);
      renderSelection();
    },
    onDoubleClick: (hit) => {
      if (hit) zoomInto(hit.index);
    },
    onContextMenu: () => goUp(),
  });
  canvas.addEventListener("mousemove", (e) => {
    if (tip.classList.contains("on")) placeTip(e.offsetX, e.offsetY);
  });
  window.addEventListener("keydown", (e) => {
    if (e.key === "Backspace" || e.key === "Escape") goUp();
  });
  renderUsage();
  show(await load(manifest.root));
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
}

// gancho para las capturas automáticas (Playwright)
const ready = main();
const lab = {
  ready,
  get view() {
    return view;
  },
  get current() {
    return { id: cur.meta.id, name: cur.side.name, count: cur.side.count };
  },
  /** Índice del primer rectángulo con ese nombre. */
  find(name: string): number {
    return cur.side.names.indexOf(name);
  },
  /** Centro de un rectángulo en px de la página. */
  center(i: number): [number, number] {
    const [x, y, w, h] = view.screenRect(i);
    const b = view.canvas.getBoundingClientRect();
    return [b.left + x + w / 2, b.top + y + h / 2];
  },
  select(i: number) {
    selectedIndex = i;
    view.setSelected(i >= 0 ? cur.layout.ids[i] : null);
    renderSelection();
  },
  highlight(c: number | null) {
    sticky = c;
    highlight(c);
  },
  zoomInto,
  goUp,
  /** Espera a que no haya animaciones. */
  async idle(): Promise<void> {
    await ready;
    for (let k = 0; k < 400 && view.busy; k++) await new Promise((r) => setTimeout(r, 16));
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  },
};
(window as unknown as { __lab: typeof lab }).__lab = lab;
