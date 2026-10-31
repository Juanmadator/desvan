<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { api, FLAGS, type Row } from "../lib/api";
  import { bytes, pct, count } from "../lib/format";
  import { catColor } from "../lib/categories";

  let { oncontext }: { oncontext?: (e: MouseEvent, row: Row) => void } = $props();

  interface TRow {
    row: Row;
    depth: number;
    open: boolean;
    parentAlloc: number;
    /** fila "mostrar más" */
    more?: { parent: number; offset: number };
  }

  const H = 26;
  const PAGE = 400;
  let rows = $state<TRow[]>([]);
  let scrollTop = $state(0);
  let viewH = $state(600);
  let box: HTMLDivElement;

  const metricOf = (r: Row) => (app.metric === "alloc" ? r.alloc : r.size);

  async function loadRoot() {
    if (!app.summary) return;
    const root = await api.node(app.summary.root);
    rows = [{ row: root, depth: 0, open: false, parentAlloc: metricOf(root) }];
    await open(0);
  }

  async function open(i: number) {
    const r = rows[i];
    if (!r || r.open || !(r.row.flags & FLAGS.DIR)) return;
    const kids = await api.children(r.row.id, 0, PAGE);
    const inserted: TRow[] = kids.map((k) => ({ row: k, depth: r.depth + 1, open: false, parentAlloc: metricOf(r.row) }));
    if (r.row.kids > PAGE) inserted.push({ row: { ...r.row, name: "" }, depth: r.depth + 1, open: false, parentAlloc: 0, more: { parent: r.row.id, offset: PAGE } });
    rows = [...rows.slice(0, i), { ...r, open: true }, ...inserted, ...rows.slice(i + 1)];
  }

  function close(i: number) {
    const r = rows[i];
    let j = i + 1;
    while (j < rows.length && rows[j].depth > r.depth) j++;
    rows = [...rows.slice(0, i), { ...r, open: false }, ...rows.slice(j)];
  }

  async function more(i: number) {
    const m = rows[i].more!;
    const parent = rows.find((x) => x.row.id === m.parent && !x.more);
    const kids = await api.children(m.parent, m.offset, PAGE);
    const depth = rows[i].depth;
    const add: TRow[] = kids.map((k) => ({ row: k, depth, open: false, parentAlloc: parent ? metricOf(parent.row) : 0 }));
    if (kids.length === PAGE) add.push({ ...rows[i], more: { parent: m.parent, offset: m.offset + PAGE } });
    rows = [...rows.slice(0, i), ...add, ...rows.slice(i + 1)];
  }

  function toggle(i: number) {
    if (rows[i].open) close(i);
    else void open(i);
  }

  /** Despliega los antepasados de un nodo y lo trae a la vista. */
  async function reveal(id: number) {
    const chain = await api.ancestors(id);
    for (const a of chain.slice(0, -1)) {
      const i = rows.findIndex((r) => r.row.id === a.id && !r.more);
      if (i >= 0 && !rows[i].open) await open(i);
    }
    const i = rows.findIndex((r) => r.row.id === id && !r.more);
    if (i >= 0 && box) {
      const top = i * H;
      if (top < box.scrollTop || top > box.scrollTop + viewH - H * 2) box.scrollTo({ top: Math.max(0, top - viewH / 3) });
    }
  }

  // nuevo escaneo o borrado: recargar conservando lo desplegado
  let lastKey = "";
  $effect(() => {
    const key = `${app.summary?.id}:${app.version}:${app.metric}`;
    if (key === lastKey) return;
    lastKey = key;
    const openIds = new Set(rows.filter((r) => r.open).map((r) => r.row.id));
    void loadRoot().then(async () => {
      for (const id of openIds) {
        const i = rows.findIndex((r) => r.row.id === id && !r.more);
        if (i >= 0 && !rows[i].open) await open(i);
      }
    });
  });

  $effect(() => {
    const id = app.selected;
    if (id !== null) void reveal(id);
  });

  const first = $derived(Math.max(0, Math.floor(scrollTop / H) - 8));
  const last = $derived(Math.min(rows.length, Math.ceil((scrollTop + viewH) / H) + 8));

  function onKey(e: KeyboardEvent) {
    const i = rows.findIndex((r) => r.row.id === app.selected && !r.more);
    if (i < 0) return;
    if (e.key === "ArrowDown" && i + 1 < rows.length) void app.select(rows[i + 1].row.id);
    else if (e.key === "ArrowUp" && i > 0) void app.select(rows[i - 1].row.id);
    else if (e.key === "ArrowRight") void open(i);
    else if (e.key === "ArrowLeft") {
      if (rows[i].open) close(i);
      else {
        for (let j = i - 1; j >= 0; j--) if (rows[j].depth < rows[i].depth) return void app.select(rows[j].row.id);
      }
    } else if (e.key === "Enter" && rows[i].row.flags & FLAGS.DIR) void app.zoom(rows[i].row.id);
    else return;
    e.preventDefault();
  }
</script>

<div class="tree">
  <div class="head">
    <span>Nombre</span>
    <span class="r">{app.metric === "alloc" ? "En disco" : "Tamaño"}</span>
    <span class="r pc">%</span>
  </div>
  <div
    class="body scroll"
    bind:this={box}
    bind:clientHeight={viewH}
    onscroll={(e) => (scrollTop = (e.currentTarget as HTMLDivElement).scrollTop)}
    tabindex="0"
    role="tree"
    aria-label="Carpetas"
    onkeydown={onKey}
  >
    <div style="height:{rows.length * H}px; position:relative">
      {#each rows.slice(first, last) as r, k (r.more ? `m${r.more.parent}-${r.more.offset}` : r.row.id)}
        {@const i = first + k}
        {#if r.more}
          <button class="row more" style="top:{i * H}px; padding-left:{12 + r.depth * 14}px" onclick={() => more(i)}>Mostrar más…</button>
        {:else}
          {@const isDir = (r.row.flags & FLAGS.DIR) !== 0}
          {@const frac = r.parentAlloc > 0 ? metricOf(r.row) / r.parentAlloc : 1}
          <div
            class="row"
            class:sel={app.selected === r.row.id}
            class:focus={app.focus === r.row.id}
            class:dim={(r.row.flags & (FLAGS.HARDLINK | FLAGS.METAFILE | FLAGS.SYNTHETIC)) !== 0}
            style="top:{i * H}px"
            role="treeitem"
            aria-selected={app.selected === r.row.id}
            aria-expanded={isDir ? r.open : undefined}
            tabindex="-1"
            onclick={() => app.select(r.row.id)}
            ondblclick={() => (isDir ? app.zoom(r.row.id) : app.openFile?.(r.row.id))}
            oncontextmenu={(e) => {
              e.preventDefault();
              void app.select(r.row.id);
              oncontext?.(e, r.row);
            }}
            onkeydown={() => {}}
          >
            <span class="name" style="padding-left:{6 + r.depth * 14}px">
              {#if isDir && r.row.kids > 0}
                <button class="chev" class:open={r.open} onclick={(e) => (e.stopPropagation(), toggle(i))} aria-label={r.open ? "Plegar" : "Desplegar"}>▸</button>
              {:else}
                <span class="chev ph"></span>
              {/if}
              {#if isDir}
                <span class="ico dir" aria-hidden="true"></span>
              {:else}
                <span class="ico" style="background:{catColor(r.row.cat)}" aria-hidden="true"></span>
              {/if}
              <span class="txt" title={r.row.name}>{r.row.name}</span>
              {#if r.row.flags & FLAGS.HARDLINK}<span class="tag" title="Enlace duro: no suma en los totales">enlace</span>{/if}
              {#if r.row.flags & FLAGS.REPARSE}<span class="tag" title="Unión o enlace simbólico: no se recorre">→</span>{/if}
              {#if r.row.flags & FLAGS.DENIED}<span class="tag warn" title="Sin permiso de lectura">🔒</span>{/if}
            </span>
            <span class="size">
              <span class="minibar"><span style="width:{Math.max(1, frac * 100)}%"></span></span>
              {bytes(metricOf(r.row))}
            </span>
            <span class="pc" title="{count(r.row.files)} archivos">{pct(frac, 0)}</span>
          </div>
        {/if}
      {/each}
    </div>
  </div>
</div>

<style>
  .tree {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .head {
    display: grid;
    grid-template-columns: 1fr 116px 44px;
    gap: 6px;
    padding: 6px 10px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--faint);
    border-bottom: 1px solid var(--line);
  }
  .r {
    text-align: right;
  }
  .body {
    flex: 1;
    min-height: 0;
    outline: none;
  }
  .row {
    position: absolute;
    left: 0;
    right: 0;
    height: 26px;
    display: grid;
    grid-template-columns: 1fr 116px 44px;
    gap: 6px;
    align-items: center;
    padding-right: 10px;
    cursor: default;
    border-radius: 6px;
    margin: 0 4px;
  }
  .row:hover {
    background: #161a23;
  }
  .row.sel {
    background: #1f2533;
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .row.focus .txt {
    color: var(--accent-2);
  }
  .row.dim {
    opacity: 0.55;
  }
  .row.more {
    border: 0;
    background: none;
    color: var(--accent);
    text-align: left;
    display: block;
    font-size: 12px;
  }
  .name {
    display: flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
  }
  .chev {
    width: 16px;
    height: 16px;
    border: 0;
    background: none;
    color: var(--faint);
    padding: 0;
    font-size: 10px;
    transition: transform 0.12s;
    flex: none;
  }
  .chev.open {
    transform: rotate(90deg);
    color: var(--muted);
  }
  .chev.ph {
    display: inline-block;
  }
  .ico {
    width: 9px;
    height: 9px;
    border-radius: 2px;
    flex: none;
  }
  .ico.dir {
    width: 12px;
    height: 9px;
    border-radius: 2px 3px 2px 2px;
    background: linear-gradient(180deg, #e8c27a, #c9973f);
  }
  .txt {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tag {
    font-size: 10px;
    color: var(--muted);
    border: 1px solid var(--line-2);
    border-radius: 4px;
    padding: 0 4px;
    flex: none;
  }
  .tag.warn {
    border-color: #5a2834;
  }
  .size {
    display: flex;
    align-items: center;
    gap: 6px;
    justify-content: flex-end;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .minibar {
    width: 34px;
    height: 4px;
    border-radius: 2px;
    background: #222836;
    overflow: hidden;
    flex: none;
  }
  .minibar span {
    display: block;
    height: 100%;
    background: var(--accent);
    opacity: 0.85;
  }
  .pc {
    text-align: right;
    color: var(--muted);
    font-size: 12px;
  }
</style>
