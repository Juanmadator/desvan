<script lang="ts">
  import { app } from "../../lib/app.svelte";
  import { api, type JunkHit } from "../../lib/api";
  import { bytes, count } from "../../lib/format";

  let hits = $state<JunkHit[] | null>(null);
  let openRule = $state<string | null>(null);
  $effect(() => {
    void app.version;
    if (app.summary) void api.junk().then((h) => (hits = h.filter((x) => x.bytes > 0).sort((a, b) => b.bytes - a.bytes)));
  });

  const groups = $derived.by(() => {
    const g: { risk: JunkHit["risk"]; title: string; hint: string; items: JunkHit[] }[] = [
      { risk: "seguro", title: "Seguro de borrar", hint: "Cachés y temporales que se regeneran solos.", items: [] },
      { risk: "revisar", title: "Revísalo antes", hint: "Suele sobrar, pero puede que lo quieras.", items: [] },
      { risk: "informativo", title: "Para que lo sepas", hint: "No se borra a mano, pero se puede reducir.", items: [] },
    ];
    for (const h of hits ?? []) g.find((x) => x.risk === h.risk)?.items.push(h);
    return g.filter((x) => x.items.length);
  });
  const recoverable = $derived((hits ?? []).filter((h) => h.risk !== "informativo").reduce((a, h) => a + h.bytes, 0));
</script>

{#if hits === null}
  <p class="muted">Buscando basura…</p>
{:else if hits.length === 0}
  <p class="muted">No hay nada evidente que limpiar. ¡Bien!</p>
{:else}
  <div class="hero">
    <span class="muted">Puedes recuperar hasta</span>
    <strong>{bytes(recoverable)}</strong>
  </div>
  {#each groups as g (g.risk)}
    <h3 class={g.risk}>{g.title}</h3>
    <p class="hint muted">{g.hint}</p>
    {#each g.items as h (h.rule)}
      <article class="rule">
        <button class="head" onclick={() => (openRule = openRule === h.rule ? null : h.rule)} aria-expanded={openRule === h.rule}>
          <span class="t">{h.title}</span>
          <span class="b">{bytes(h.bytes)}</span>
        </button>
        <p class="d">{h.description}</p>
        {#if openRule === h.rule}
          <ul class="paths scroll">
            {#each h.nodes.slice(0, 40) as n (n.id)}
              <li><button onclick={() => app.show(n.id)} title={n.path}><span>{n.path}</span><span class="muted">{bytes(n.alloc)}</span></button></li>
            {/each}
            {#if h.nodes.length > 40}<li class="muted more">y {count(h.nodes.length - 40)} más…</li>{/if}
          </ul>
        {/if}
        <div class="acts">
          <button class="btn sm ghost" onclick={() => (openRule = openRule === h.rule ? null : h.rule)}>{openRule === h.rule ? "Ocultar" : `Ver ${count(h.nodes.length)}`}</button>
          {#if h.risk !== "informativo"}
            <button class="btn sm danger" onclick={() => app.askDelete(h.nodes.map((n) => n.id))}>Enviar a la Papelera…</button>
          {/if}
        </div>
      </article>
    {/each}
  {/each}
{/if}

<style>
  .hero {
    display: flex;
    flex-direction: column;
    margin-bottom: 14px;
  }
  .hero strong {
    font-size: 30px;
    letter-spacing: -0.02em;
  }
  h3 {
    margin: 16px 0 2px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  h3.seguro {
    color: var(--ok);
  }
  h3.revisar {
    color: var(--accent);
  }
  h3.informativo {
    color: var(--muted);
  }
  .hint {
    margin: 0 0 8px;
    font-size: 12px;
  }
  .rule {
    border: 1px solid var(--line);
    background: var(--panel);
    border-radius: 12px;
    padding: 10px 12px;
    margin-bottom: 8px;
  }
  .head {
    width: 100%;
    display: flex;
    justify-content: space-between;
    gap: 10px;
    background: none;
    border: 0;
    padding: 0;
    text-align: left;
    font-weight: 600;
  }
  .b {
    white-space: nowrap;
  }
  .d {
    margin: 4px 0 8px;
    font-size: 12px;
    color: var(--muted);
  }
  .paths {
    list-style: none;
    padding: 0;
    margin: 0 0 8px;
    max-height: 200px;
    border-top: 1px solid var(--line);
  }
  .paths button {
    width: 100%;
    display: flex;
    justify-content: space-between;
    gap: 8px;
    background: none;
    border: 0;
    padding: 4px 2px;
    font-size: 12px;
    text-align: left;
  }
  .paths button span:first-child {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .paths button:hover {
    color: var(--accent-2);
  }
  .more {
    font-size: 12px;
    padding: 4px 2px;
  }
  .acts {
    display: flex;
    gap: 6px;
    justify-content: flex-end;
  }
</style>
