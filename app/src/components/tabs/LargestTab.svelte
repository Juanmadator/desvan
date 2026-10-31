<script lang="ts">
  import { app } from "../../lib/app.svelte";
  import { api, type Hit } from "../../lib/api";
  import { ago, bytes } from "../../lib/format";
  import { catColor } from "../../lib/categories";

  let list = $state<Hit[]>([]);
  let loading = $state(false);
  $effect(() => {
    void app.version;
    const f = app.focus;
    if (!app.summary) return;
    loading = true;
    void api.largest(f, 150).then((l) => {
      list = l;
      loading = false;
    });
  });
</script>

<h3>Archivos más grandes {loading ? "…" : ""}</h3>
<ul>
  {#each list as h (h.id)}
    <li>
      <button class:sel={app.selected === h.id} onclick={() => app.show(h.id)} title={h.path}>
        <span class="sw" style="background:{catColor(h.cat)}"></span>
        <span class="nm">{h.name}<small>{h.path}</small></span>
        <span class="sz">{bytes(app.metric === "alloc" ? h.alloc : h.size)}<small>{ago(h.mtime)}</small></span>
      </button>
    </li>
  {/each}
</ul>

<style>
  h3 {
    margin: 0 0 10px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  button {
    width: 100%;
    display: grid;
    grid-template-columns: 10px 1fr auto;
    gap: 8px;
    align-items: center;
    text-align: left;
    background: none;
    border: 0;
    padding: 6px;
    border-radius: 8px;
  }
  button:hover {
    background: var(--raise);
  }
  button.sel {
    background: #1f2533;
  }
  .sw {
    width: 9px;
    height: 9px;
    border-radius: 2px;
  }
  .nm,
  .sz {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .nm {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  small {
    color: var(--faint);
    font-size: 11px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sz {
    text-align: right;
    font-weight: 600;
  }
  .sz small {
    font-weight: 400;
  }
</style>
