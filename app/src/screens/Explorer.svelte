<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { FLAGS, type Row } from "../lib/api";
  import Toolbar from "../components/Toolbar.svelte";
  import TreeList from "../components/TreeList.svelte";
  import TreemapPane from "../components/TreemapPane.svelte";
  import Details from "../components/Details.svelte";
  import StatusBar from "../components/StatusBar.svelte";
  import ContextMenu from "../components/ContextMenu.svelte";
  import ConfirmDelete from "../components/ConfirmDelete.svelte";

  let menu = $state<{ x: number; y: number; row: Row } | null>(null);
  let leftW = $state(360);
  let rightW = $state(380);

  function oncontext(e: MouseEvent, row: Row) {
    menu = { x: e.clientX, y: e.clientY, row };
  }

  function onKey(e: KeyboardEvent) {
    const t = e.target as HTMLElement;
    if (t && (t.tagName === "INPUT" || t.tagName === "SELECT")) return;
    if (e.key === "Backspace" || (e.altKey && e.key === "ArrowLeft")) {
      e.preventDefault();
      void app.zoomOut();
    } else if (e.key === "Delete" && app.selected !== null && app.selectedInfo && !(app.selectedInfo.flags & (FLAGS.SYNTHETIC | FLAGS.METAFILE))) {
      e.preventDefault();
      void app.askDelete([app.selected]);
    } else if (e.key === "Escape") {
      void app.select(null);
    }
  }

  // arrastrar los separadores para cambiar el ancho de los paneles
  function drag(side: "l" | "r") {
    return (e: PointerEvent) => {
      const startX = e.clientX;
      const start = side === "l" ? leftW : rightW;
      const move = (ev: PointerEvent) => {
        const d = ev.clientX - startX;
        if (side === "l") leftW = Math.max(240, Math.min(620, start + d));
        else rightW = Math.max(300, Math.min(620, start - d));
      };
      const up = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
    };
  }
</script>

<svelte:window onkeydown={onKey} onmouseup={(e) => e.button === 3 && app.zoomOut()} />

<div class="ex">
  <Toolbar />
  <div class="cols" style="grid-template-columns:{leftW}px 4px 1fr 4px {rightW}px">
    <aside class="left"><TreeList {oncontext} /></aside>
    <div class="split" role="separator" aria-orientation="vertical" onpointerdown={drag("l")}></div>
    <section class="center"><TreemapPane {oncontext} /></section>
    <div class="split" role="separator" aria-orientation="vertical" onpointerdown={drag("r")}></div>
    <aside class="right"><Details /></aside>
  </div>
  <StatusBar />
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} row={menu.row} onclose={() => (menu = null)} />
{/if}
{#if app.pendingDelete}
  <ConfirmDelete />
{/if}

<style>
  .ex {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .cols {
    flex: 1;
    min-height: 0;
    display: grid;
  }
  .left,
  .right {
    background: var(--panel);
    min-width: 0;
    min-height: 0;
  }
  .center {
    min-width: 0;
    min-height: 0;
    position: relative;
    background: #0d0f14;
  }
  .split {
    background: var(--line);
    cursor: col-resize;
    transition: background 0.15s;
  }
  .split:hover {
    background: var(--accent);
  }
</style>
