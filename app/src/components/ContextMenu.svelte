<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { FLAGS, type Row } from "../lib/api";

  let { x, y, row, onclose }: { x: number; y: number; row: Row; onclose: () => void } = $props();
  const isDir = $derived((row.flags & FLAGS.DIR) !== 0);
  const protectedNode = $derived((row.flags & (FLAGS.SYNTHETIC | FLAGS.METAFILE)) !== 0 || row.id === app.summary?.root);

  function act(f: () => unknown) {
    onclose();
    void f();
  }

  // que no se salga de la ventana
  const left = $derived(Math.min(x, window.innerWidth - 230));
  const top = $derived(Math.min(y, window.innerHeight - 260));
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />
<div class="veil" role="presentation" onmousedown={onclose} oncontextmenu={(e) => (e.preventDefault(), onclose())}></div>
<div class="menu" style="left:{left}px; top:{top}px" role="menu">
  <div class="title" title={row.name}>{row.name}</div>
  {#if isDir}
    <button role="menuitem" onclick={() => act(() => app.zoom(row.id))}>Ampliar aquí <span class="kbd">doble clic</span></button>
  {:else}
    <button role="menuitem" onclick={() => act(() => app.openFile(row.id))}>Abrir</button>
  {/if}
  <button role="menuitem" onclick={() => act(() => app.reveal(row.id))}>Mostrar en el Explorador</button>
  <button role="menuitem" onclick={() => act(() => app.copyPath(row.id))}>Copiar ruta</button>
  <button role="menuitem" onclick={() => act(() => app.zoomOut())}>Subir un nivel <span class="kbd">Retroceso</span></button>
  <hr />
  <button role="menuitem" class="danger" disabled={protectedNode} onclick={() => act(() => app.askDelete([row.id]))}>Enviar a la Papelera… <span class="kbd">Supr</span></button>
</div>

<style>
  .veil {
    position: fixed;
    inset: 0;
    z-index: 80;
  }
  .menu {
    position: fixed;
    z-index: 81;
    min-width: 220px;
    background: var(--panel-2);
    border: 1px solid var(--line-2);
    border-radius: 12px;
    box-shadow: var(--shadow);
    padding: 5px;
    display: flex;
    flex-direction: column;
    animation: pop 0.1s ease-out;
  }
  @keyframes pop {
    from {
      transform: scale(0.97);
      opacity: 0;
    }
  }
  .title {
    padding: 6px 10px 4px;
    font-size: 11px;
    color: var(--faint);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 260px;
  }
  button {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
    background: none;
    border: 0;
    text-align: left;
    padding: 7px 10px;
    border-radius: 7px;
  }
  button:hover:not(:disabled) {
    background: var(--raise);
  }
  .danger {
    color: #ffb3c0;
  }
  hr {
    border: 0;
    border-top: 1px solid var(--line);
    margin: 4px 2px;
  }
</style>
