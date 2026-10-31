<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { bytes, count, duration } from "../lib/format";

  const s = $derived(app.summary!);
</script>

<footer class="status">
  {#if s.method === "mft"}
    <span class="badge fast" title="Lectura directa de la tabla maestra de archivos de NTFS">⚡ MFT</span>
  {:else}
    <span class="badge" title="Recorrido de carpetas">Recorrido</span>
  {/if}
  <span><strong>{count(s.files)}</strong> archivos</span>
  <span><strong>{count(s.dirs)}</strong> carpetas</span>
  <span><strong>{bytes(s.alloc)}</strong> en disco</span>
  <span class="muted">analizado en {duration(s.stats.elapsed_ms)}</span>
  {#if s.stats.denied > 0}<span class="warn" title="Carpetas sin permiso de lectura">🔒 {count(s.stats.denied)} sin acceso</span>{/if}
  <span class="grow"></span>
  {#if s.volume && s.volume.total > 0}
    <span class="muted">{bytes(s.volume.free)} libres de {bytes(s.volume.total)}</span>
  {/if}
  <span class="muted" title="Memoria que ocupa el árbol">{bytes(s.tree_bytes)} en memoria</span>
</footer>

<style>
  .status {
    height: 28px;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 0 12px;
    border-top: 1px solid var(--line);
    background: var(--bg-2);
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
  }
  .grow {
    flex: 1;
  }
  .warn {
    color: #ffcf8a;
  }
</style>
