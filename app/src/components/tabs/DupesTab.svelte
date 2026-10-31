<script lang="ts">
  import { onDestroy } from "svelte";
  import { app } from "../../lib/app.svelte";
  import { api, events, type DupGroup, type DupProgress } from "../../lib/api";
  import { ago, bytes, count, pct } from "../../lib/format";

  let minSize = $state(1 << 20);
  let running = $state(false);
  let progress = $state<DupProgress | null>(null);
  let groups = $state<DupGroup[]>([]);
  let scopeName = $state("");
  const unsubs: (() => void)[] = [];

  async function start() {
    groups = [];
    progress = null;
    running = true;
    const info = await api.node(app.focus);
    scopeName = info.name;
    if (!unsubs.length) {
      unsubs.push(
        await events.dupesProgress((p) => {
          progress = p;
          if (p.finished) running = false;
        }),
      );
      unsubs.push(
        await events.dupesGroup((g) => {
          groups = [...groups, g].sort((a, b) => b.wasted - a.wasted);
        }),
      );
    }
    await api.dupesStart(app.focus, minSize);
  }

  async function stop() {
    await api.dupesCancel();
    running = false;
  }

  /** Conserva el más reciente y manda el resto a la Papelera (con confirmación). */
  function keepNewest(g: DupGroup) {
    const sorted = [...g.nodes].sort((a, b) => b.mtime - a.mtime);
    void app.askDelete(sorted.slice(1).map((n) => n.id));
  }

  onDestroy(() => unsubs.forEach((u) => u()));
  const wasted = $derived(groups.reduce((a, g) => a + g.wasted, 0));
</script>

<div class="cfg">
  <label>
    Tamaño mínimo
    <select bind:value={minSize} disabled={running}>
      <option value={1 << 20}>1 MB</option>
      <option value={10 << 20}>10 MB</option>
      <option value={100 << 20}>100 MB</option>
      <option value={1 << 30}>1 GB</option>
    </select>
  </label>
  {#if running}
    <button class="btn sm" onclick={stop}>Detener</button>
  {:else}
    <button class="btn sm primary" onclick={start}>Buscar duplicados</button>
  {/if}
</div>
<p class="muted note">Compara el contenido real (no solo el nombre) dentro de la carpeta del mapa. Primero agrupa por tamaño, después compara el principio y el final de cada archivo, y solo entonces lee los candidatos enteros.</p>

{#if progress}
  <div class="prog">
    <div class="track"><div style="width:{(progress.done / Math.max(1, progress.total)) * 100}%"></div></div>
    <span class="muted">{progress.finished ? "Terminado" : `Comparando… ${pct(progress.done / Math.max(1, progress.total), 0)}`} · {count(groups.length)} grupos · {bytes(wasted)} repetidos {scopeName ? `en ${scopeName}` : ""}</span>
  </div>
{/if}

{#each groups as g (g.hash)}
  <article class="group">
    <header>
      <strong>{g.nodes.length} copias de {bytes(g.size)}</strong>
      <span class="w">{bytes(g.wasted)} de sobra</span>
    </header>
    <ul>
      {#each g.nodes as n (n.id)}
        <li>
          <button class="path" onclick={() => app.show(n.id)} title={n.path}>{n.path}</button>
          <span class="muted">{ago(n.mtime)}</span>
        </li>
      {/each}
    </ul>
    <div class="acts">
      <button class="btn sm danger" onclick={() => keepNewest(g)}>Conservar el más reciente…</button>
    </div>
  </article>
{/each}

<style>
  .cfg {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  label {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--muted);
  }
  select {
    background: var(--panel);
    color: var(--text);
    border: 1px solid var(--line-2);
    border-radius: 7px;
    padding: 4px 6px;
  }
  .note {
    font-size: 12px;
    margin: 8px 0 12px;
  }
  .prog {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 12px;
    font-size: 12px;
  }
  .track {
    height: 5px;
    background: var(--raise);
    border-radius: 3px;
    overflow: hidden;
  }
  .track div {
    height: 100%;
    background: var(--teal);
    transition: width 0.2s;
  }
  .group {
    border: 1px solid var(--line);
    background: var(--panel);
    border-radius: 12px;
    padding: 10px 12px;
    margin-bottom: 8px;
  }
  .group header {
    display: flex;
    justify-content: space-between;
    gap: 8px;
  }
  .w {
    color: var(--accent);
    white-space: nowrap;
  }
  ul {
    list-style: none;
    margin: 6px 0;
    padding: 0;
  }
  li {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    font-size: 12px;
    padding: 2px 0;
  }
  .path {
    background: none;
    border: 0;
    padding: 0;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .path:hover {
    color: var(--accent-2);
  }
  .acts {
    display: flex;
    justify-content: flex-end;
  }
</style>
