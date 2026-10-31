<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { api, type Hit } from "../lib/api";
  import { bytes } from "../lib/format";
  import { catColor } from "../lib/categories";
  import Logo from "./Logo.svelte";

  let crumbs = $state<{ id: number; name: string }[]>([]);
  let query = $state("");
  let results = $state<Hit[]>([]);
  let searching = $state(false);
  let open = $state(false);
  let input: HTMLInputElement;
  let timer = 0;

  $effect(() => {
    const f = app.focus;
    void app.version;
    if (app.summary) void api.ancestors(f).then((c) => (crumbs = c));
  });

  function onInput() {
    clearTimeout(timer);
    const q = query.trim();
    if (q.length < 2) {
      results = [];
      return;
    }
    timer = window.setTimeout(async () => {
      searching = true;
      results = await api.search(q, 60);
      searching = false;
      open = true;
    }, 160);
  }

  async function pick(h: Hit) {
    open = false;
    await app.show(h.id);
  }

  export function focusSearch() {
    input?.focus();
    input?.select();
  }

  function onWindowKey(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
      e.preventDefault();
      focusSearch();
    }
  }
</script>

<svelte:window onkeydown={onWindowKey} />

<header class="bar">
  <button class="home" onclick={() => app.backHome()} title="Volver a las unidades">
    <Logo size={22} />
  </button>

  <nav class="crumbs" aria-label="Ruta">
    {#each crumbs as c, i (c.id)}
      {#if i > 0}<span class="sep">›</span>{/if}
      <button class="crumb" class:cur={i === crumbs.length - 1} onclick={() => app.zoom(c.id)}>{c.name}</button>
    {/each}
  </nav>

  <div class="search">
    <span class="lens" aria-hidden="true">⌕</span>
    <input
      bind:this={input}
      bind:value={query}
      oninput={onInput}
      onfocus={() => results.length && (open = true)}
      onblur={() => setTimeout(() => (open = false), 150)}
      onkeydown={(e) => {
        if (e.key === "Escape") {
          query = "";
          results = [];
          input.blur();
        } else if (e.key === "Enter" && results[0]) pick(results[0]);
      }}
      placeholder="Buscar archivos y carpetas…"
      spellcheck="false"
    />
    <span class="kbd">Ctrl F</span>
    {#if open && (results.length || searching)}
      <div class="results scroll" role="listbox">
        {#if searching && !results.length}
          <div class="empty">Buscando…</div>
        {/if}
        {#each results as h (h.id)}
          <button class="hit" onmousedown={(e) => e.preventDefault()} onclick={() => pick(h)}>
            <span class="dot" style="background:{h.cat === 255 ? '#c9973f' : catColor(h.cat)}"></span>
            <span class="nm">{h.name}<small>{h.path}</small></span>
            <span class="sz">{bytes(app.metric === "alloc" ? h.alloc : h.size)}</span>
          </button>
        {/each}
      </div>
    {/if}
  </div>

  <div class="seg" role="radiogroup" aria-label="Medida">
    <button role="radio" aria-checked={app.metric === "alloc"} class:on={app.metric === "alloc"} onclick={() => (app.metric = "alloc")} title="Clústeres ocupados en el disco">En disco</button>
    <button role="radio" aria-checked={app.metric === "size"} class:on={app.metric === "size"} onclick={() => (app.metric = "size")} title="Tamaño de los datos">Tamaño</button>
  </div>

  <button class="btn sm" onclick={() => app.scan(app.target)} title="Volver a analizar">↻ Reanalizar</button>
</header>

<style>
  .bar {
    height: 48px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 12px;
    border-bottom: 1px solid var(--line);
    background: var(--bg-2);
  }
  .home {
    background: none;
    border: 0;
    padding: 4px;
    border-radius: 8px;
    display: grid;
  }
  .home:hover {
    background: var(--raise);
  }
  .crumbs {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
    flex: 1;
    overflow: hidden;
  }
  .crumb {
    background: none;
    border: 0;
    padding: 4px 6px;
    border-radius: 6px;
    color: var(--muted);
    white-space: nowrap;
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .crumb:hover {
    background: var(--raise);
    color: var(--text);
  }
  .crumb.cur {
    color: var(--text);
    font-weight: 600;
  }
  .sep {
    color: var(--faint);
  }
  .search {
    position: relative;
    display: flex;
    align-items: center;
    width: min(380px, 32vw);
    height: 32px;
    border: 1px solid var(--line-2);
    border-radius: 9px;
    background: var(--panel);
    padding: 0 8px;
    gap: 6px;
  }
  .search:focus-within {
    border-color: #5a4a2a;
    box-shadow: 0 0 0 3px rgba(243, 167, 59, 0.12);
  }
  .lens {
    color: var(--faint);
    font-size: 15px;
  }
  .search input {
    flex: 1;
    min-width: 0;
    background: none;
    border: 0;
    outline: none;
  }
  .results {
    position: absolute;
    top: 38px;
    left: 0;
    right: 0;
    max-height: 60vh;
    background: var(--panel-2);
    border: 1px solid var(--line-2);
    border-radius: 12px;
    box-shadow: var(--shadow);
    padding: 4px;
    z-index: 50;
  }
  .hit {
    width: 100%;
    display: grid;
    grid-template-columns: 10px 1fr auto;
    gap: 8px;
    align-items: center;
    text-align: left;
    background: none;
    border: 0;
    padding: 6px 8px;
    border-radius: 8px;
  }
  .hit:hover {
    background: var(--raise);
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 2px;
  }
  .nm {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .nm small {
    color: var(--faint);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sz {
    color: var(--muted);
    font-size: 12px;
  }
  .empty {
    padding: 10px;
    color: var(--muted);
  }
  .seg {
    display: flex;
    background: var(--panel);
    border: 1px solid var(--line-2);
    border-radius: 9px;
    padding: 2px;
  }
  .seg button {
    border: 0;
    background: none;
    padding: 4px 10px;
    border-radius: 7px;
    color: var(--muted);
    font-size: 12px;
  }
  .seg button.on {
    background: var(--raise);
    color: var(--text);
  }
</style>
