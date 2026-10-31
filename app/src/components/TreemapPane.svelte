<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "../lib/app.svelte";
  import { api, FLAGS, type Row } from "../lib/api";
  import { bytes, pct } from "../lib/format";
  import { TreemapView, decodeLayout, CATEGORY_COLORS, FIELD, RFLAG, STRIDE, LAYOUT_HEADER_BYTES, type DecodedLayout, type TreemapHit } from "../lib/treemap";
  import { catLabel } from "../lib/categories";

  let { oncontext }: { oncontext?: (e: MouseEvent, row: Row) => void } = $props();

  let host = $state<HTMLDivElement>();
  let canvas: HTMLCanvasElement;
  let view: TreemapView | null = null;
  let current: DecodedLayout | null = null;
  let labels = new Map<number, { name: string; value: number }>();
  let tip = $state<{ x: number; y: number; name: string; value: number; cat: number; dir: boolean; files: number; frac: number } | null>(null);
  let loading = $state(false);
  let empty = $state(false);
  let reqSeq = 0;
  let pendingZoom: { index: number; id: number } | null = null;

  /** Decodifica la sección de etiquetas que el backend añade tras el layout. */
  function decodeLabels(buf: ArrayBuffer, count: number) {
    const map = new Map<number, { name: string; value: number }>();
    let o = LAYOUT_HEADER_BYTES + count * STRIDE * 4 + count * 4;
    if (o + 4 > buf.byteLength) return map;
    const dv = new DataView(buf);
    const n = dv.getUint32(o, true);
    o += 4;
    const dec = new TextDecoder();
    for (let k = 0; k < n && o + 14 <= buf.byteLength; k++) {
      const index = dv.getUint32(o, true);
      const value = Number(dv.getBigUint64(o + 4, true));
      const len = dv.getUint16(o + 12, true);
      const name = dec.decode(new Uint8Array(buf, o + 14, len));
      map.set(index, { name, value });
      o += 14 + len;
    }
    return map;
  }

  async function load(animateFrom: { index: number } | null = null, out = false) {
    if (!view || !app.summary) return;
    const w = Math.max(1, Math.floor(canvas.clientWidth));
    const h = Math.max(1, Math.floor(canvas.clientHeight));
    const seq = ++reqSeq;
    const focus = app.focus;
    const run = async () => {
      loading = true;
      const buf = await api.layout(focus, w, h, app.metric, 4);
      if (seq !== reqSeq || !view) return;
      const d = decodeLayout(buf);
      current = d;
      labels = decodeLabels(buf, d.count);
      empty = d.ids.length === 0;
      view.setData(d.rects, d.ids, d.meta);
      applyHighlight();
      view.setSelected(app.selected);
      loading = false;
    };
    if (animateFrom) {
      const r = view.rectAt(animateFrom.index);
      view.zoomTo([r.x, r.y, r.w, r.h], () => void run());
    }
    else if (out) view.zoomTo(null, () => void run());
    else await run();
  }

  function applyHighlight() {
    if (!view || !current) return;
    const c = app.highlightCat;
    const r = current.rects;
    view.setHighlight(c === null ? null : (i) => r[i * STRIDE + FIELD.CAT] === c);
  }

  let hoverSeq = 0;
  async function onHover(hit: TreemapHit | null, ev: MouseEvent | null) {
    if (!hit || !ev || !current) {
      tip = null;
      return;
    }
    const rect = host!.getBoundingClientRect();
    const x = ev.clientX - rect.left;
    const y = ev.clientY - rect.top;
    const known = labels.get(hit.index);
    const cat = current.rects[hit.index * STRIDE + FIELD.CAT];
    const rest = (current.rects[hit.index * STRIDE + FIELD.FLAGS] & RFLAG.REST) !== 0;
    if (tip && known) {
      tip = { ...tip, x, y };
    }
    const seq = ++hoverSeq;
    const info = await api.node(hit.id);
    if (seq !== hoverSeq) return;
    tip = {
      x,
      y,
      name: rest ? `Archivos pequeños de ${info.name}` : info.name,
      value: app.metric === "alloc" ? info.alloc : info.size,
      cat,
      dir: (info.flags & FLAGS.DIR) !== 0,
      files: info.files,
      frac: info.of_root,
    };
  }

  onMount(() => {
    view = new TreemapView(canvas, {
      palette: CATEGORY_COLORS,
      labelFor: (index) => {
        const l = labels.get(index);
        return l ? { name: l.name, detail: bytes(l.value) } : null;
      },
    });
    view.onHover = (hit, ev) => void onHover(hit, ev);
    view.onClick = (hit) => {
      if (hit) void app.select(hit.id);
    };
    // doble clic: ampliar la carpeta (o la carpeta de un «resto»); en un archivo no se abre nada
    view.onDoubleClick = (hit) => {
      if (!hit || !current) return;
      const f = current.rects[hit.index * STRIDE + FIELD.FLAGS];
      if ((f & RFLAG.DIR) !== 0 || (f & RFLAG.REST) !== 0) {
        if (hit.id === app.focus) return;
        pendingZoom = hit;
        void app.zoom(hit.id);
      }
    };
    view.onContextMenu = async (hit, ev) => {
      if (!hit) return;
      ev.preventDefault();
      const info = await api.node(hit.id);
      void app.select(hit.id);
      oncontext?.(ev, info);
    };
    // nuevo tamaño: se recalcula el mapa (con un pequeño retardo para no saturar)
    let t = 0;
    const ro = new ResizeObserver(() => {
      clearTimeout(t);
      t = window.setTimeout(() => void load(), 120);
    });
    ro.observe(canvas);
    return () => {
      ro.disconnect();
      view?.dispose();
      view = null;
    };
  });

  // cambio de carpeta, de métrica o tras borrar: nuevo layout (con animación si es un zoom)
  let lastFocus = -1;
  let lastKey = "";
  $effect(() => {
    const key = `${app.summary?.id}:${app.focus}:${app.metric}:${app.version}`;
    if (key === lastKey || !view) return;
    const prevFocus = lastFocus;
    lastKey = key;
    lastFocus = app.focus;
    if (pendingZoom && pendingZoom.id === app.focus) {
      const z = pendingZoom;
      pendingZoom = null;
      void load(z);
    } else if (prevFocus >= 0 && current && app.focus !== prevFocus && view.indexOfId(app.focus) >= 0) {
      // ampliar desde el árbol, las migas o la búsqueda: animar si se ve en el mapa
      void load({ index: view.indexOfId(app.focus) });
    } else if (prevFocus >= 0 && app.focus !== prevFocus) {
      void load(null, true);
    } else {
      void load();
    }
  });

  $effect(() => {
    void app.highlightCat;
    applyHighlight();
  });

  $effect(() => {
    view?.setSelected(app.selected);
  });
</script>

<div class="pane" bind:this={host}>
  <canvas bind:this={canvas} class="map"></canvas>
  {#if tip}
    <div class="tip" style="left:{Math.min(tip.x + 14, (host?.clientWidth ?? 0) - 260)}px; top:{tip.y + 16}px">
      <div class="t">{tip.name}</div>
      <div class="v">{bytes(tip.value)} <span class="muted">· {pct(tip.frac)} del total</span></div>
      <div class="muted s">{tip.dir ? `Carpeta · ${tip.files.toLocaleString("es-ES")} archivos` : catLabel(tip.cat)}</div>
    </div>
  {/if}
  {#if empty && !loading}
    <div class="empty muted">Aquí no hay nada que ocupe espacio.</div>
  {/if}
</div>

<style>
  .pane {
    position: absolute;
    inset: 0;
    overflow: hidden;
  }
  .map {
    position: absolute;
    inset: 6px;
    width: calc(100% - 12px);
    height: calc(100% - 12px);
    display: block;
  }
  .tip {
    position: absolute;
    z-index: 5;
    pointer-events: none;
    max-width: 260px;
    background: rgba(18, 21, 28, 0.94);
    border: 1px solid var(--line-2);
    border-radius: 10px;
    padding: 8px 10px;
    box-shadow: var(--shadow);
    backdrop-filter: blur(6px);
  }
  .t {
    font-weight: 600;
    word-break: break-word;
  }
  .v {
    margin-top: 2px;
  }
  .s {
    font-size: 11px;
  }
  .empty {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
  }
</style>
