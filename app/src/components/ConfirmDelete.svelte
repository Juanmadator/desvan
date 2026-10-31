<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { bytes } from "../lib/format";

  const p = $derived(app.pendingDelete!);
  const total = $derived(p.items.reduce((a, i) => a + i.alloc, 0));
  let busy = $state(false);
  let error = $state("");

  async function confirm() {
    busy = true;
    error = "";
    try {
      const r = await app.remove(p.ids);
      app.pendingDelete = null;
      app.notify(`Enviado a la Papelera: ${bytes(r.freed)} liberados`);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && !busy && (app.pendingDelete = null)} />
<div class="veil" role="presentation">
  <div class="modal" role="dialog" aria-modal="true" aria-labelledby="del-title">
    <h2 id="del-title">¿Enviar a la Papelera de reciclaje?</h2>
    <p class="muted">Podrás recuperarlo desde la Papelera de Windows. Se liberarán <strong>{bytes(total)}</strong> cuando la vacíes.</p>
    <ul class="scroll">
      {#each p.items as it}
        <li><span class="nm" title={it.path}>{it.path}</span><span class="sz">{bytes(it.alloc)}</span></li>
      {/each}
    </ul>
    {#if error}<p class="err">{error}</p>{/if}
    <div class="acts">
      <button class="btn" onclick={() => (app.pendingDelete = null)} disabled={busy}>Cancelar</button>
      <button class="btn danger" onclick={confirm} disabled={busy}>{busy ? "Enviando…" : "Enviar a la Papelera"}</button>
    </div>
  </div>
</div>

<style>
  .veil {
    position: fixed;
    inset: 0;
    background: rgba(5, 6, 9, 0.65);
    backdrop-filter: blur(3px);
    display: grid;
    place-items: center;
    z-index: 90;
  }
  .modal {
    width: min(560px, 92vw);
    background: var(--panel-2);
    border: 1px solid var(--line-2);
    border-radius: 16px;
    padding: 20px;
    box-shadow: var(--shadow);
  }
  h2 {
    margin: 0 0 6px;
    font-size: 17px;
  }
  ul {
    list-style: none;
    margin: 12px 0;
    padding: 0;
    max-height: 240px;
    border: 1px solid var(--line);
    border-radius: 10px;
  }
  li {
    display: flex;
    justify-content: space-between;
    gap: 10px;
    padding: 7px 10px;
    border-bottom: 1px solid var(--line);
  }
  li:last-child {
    border-bottom: 0;
  }
  .nm {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sz {
    color: var(--muted);
    flex: none;
  }
  .err {
    color: #ffb3c0;
  }
  .acts {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
