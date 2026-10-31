<script lang="ts">
  import { app } from "../../lib/app.svelte";
  import { api, type ExtStats } from "../../lib/api";
  import { bytes, count, pct } from "../../lib/format";
  import { catColor, catLabel } from "../../lib/categories";

  let stats = $state<ExtStats | null>(null);
  $effect(() => {
    void app.version;
    const f = app.focus;
    if (app.summary) void api.extStats(f).then((s) => (stats = s));
  });
  const total = $derived(stats ? stats.cats.reduce((a, c) => a + c.alloc, 0) : 0);
</script>

{#if stats}
  <section>
    <h3>Por tipo de archivo</h3>
    <div class="stack" aria-hidden="true">
      {#each stats.cats as c (c.cat)}
        <div style="width:{(c.alloc / Math.max(1, total)) * 100}%; background:{catColor(c.cat)}" title="{catLabel(c.cat)}: {bytes(c.alloc)}"></div>
      {/each}
    </div>
    <ul class="cats">
      {#each stats.cats as c (c.cat)}
        <li>
          <button class:on={app.highlightCat === c.cat} onclick={() => (app.highlightCat = app.highlightCat === c.cat ? null : c.cat)} title="Resaltar en el mapa">
            <span class="sw" style="background:{catColor(c.cat)}"></span>
            <span class="nm">{catLabel(c.cat)}</span>
            <span class="muted">{count(c.files)}</span>
            <span class="sz">{bytes(c.alloc)}</span>
            <span class="pc muted">{pct(c.alloc / Math.max(1, total), 0)}</span>
          </button>
        </li>
      {/each}
    </ul>
    {#if app.highlightCat !== null}
      <button class="btn sm ghost" onclick={() => (app.highlightCat = null)}>Quitar resaltado</button>
    {/if}

    <h3>Extensiones</h3>
    <table>
      <thead><tr><th>Extensión</th><th class="r">Archivos</th><th class="r">Tamaño</th></tr></thead>
      <tbody>
        {#each stats.exts.slice(0, 60) as e (e.ext)}
          <tr>
            <td><span class="sw" style="background:{catColor(e.cat)}"></span>{e.ext ? `.${e.ext}` : "(sin extensión)"}</td>
            <td class="r muted">{count(e.files)}</td>
            <td class="r">{bytes(e.alloc)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </section>
{/if}

<style>
  h3 {
    margin: 0 0 10px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }
  h3:not(:first-child) {
    margin-top: 20px;
  }
  .stack {
    display: flex;
    height: 12px;
    border-radius: 6px;
    overflow: hidden;
    gap: 2px;
    margin-bottom: 10px;
  }
  .cats {
    list-style: none;
    padding: 0;
    margin: 0 0 8px;
  }
  .cats button {
    width: 100%;
    display: grid;
    grid-template-columns: 12px 1fr auto 72px 40px;
    gap: 8px;
    align-items: center;
    background: none;
    border: 0;
    padding: 5px 6px;
    border-radius: 7px;
    text-align: left;
  }
  .cats button:hover {
    background: var(--raise);
  }
  .cats button.on {
    background: #2a2213;
    box-shadow: inset 0 0 0 1px #5a4220;
  }
  .sw {
    display: inline-block;
    width: 10px;
    height: 10px;
    border-radius: 3px;
    margin-right: 6px;
    vertical-align: -1px;
  }
  .cats .sw {
    margin: 0;
  }
  .sz,
  .pc {
    text-align: right;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }
  th {
    text-align: left;
    font-weight: 500;
    color: var(--faint);
    padding: 4px 6px;
    border-bottom: 1px solid var(--line);
  }
  td {
    padding: 4px 6px;
    border-bottom: 1px solid #191d26;
  }
  .r {
    text-align: right;
  }
</style>
