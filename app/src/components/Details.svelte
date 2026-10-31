<script lang="ts">
  import { app } from "../lib/app.svelte";
  import SelectionTab from "./tabs/SelectionTab.svelte";
  import TypesTab from "./tabs/TypesTab.svelte";
  import LargestTab from "./tabs/LargestTab.svelte";
  import JunkTab from "./tabs/JunkTab.svelte";
  import DupesTab from "./tabs/DupesTab.svelte";

  const tabs = [
    { id: "seleccion", label: "Detalles" },
    { id: "tipos", label: "Tipos" },
    { id: "mayores", label: "Mayores" },
    { id: "limpieza", label: "Limpieza" },
    { id: "duplicados", label: "Duplicados" },
  ] as const;
</script>

<div class="details">
  <div class="tabs" role="tablist">
    {#each tabs as t (t.id)}
      <button role="tab" aria-selected={app.tab === t.id} class:on={app.tab === t.id} onclick={() => (app.tab = t.id)}>{t.label}</button>
    {/each}
  </div>
  <div class="body scroll">
    {#if app.tab === "seleccion"}
      <SelectionTab />
    {:else if app.tab === "tipos"}
      <TypesTab />
    {:else if app.tab === "mayores"}
      <LargestTab />
    {:else if app.tab === "limpieza"}
      <JunkTab />
    {:else}
      <DupesTab />
    {/if}
  </div>
</div>

<style>
  .details {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .tabs {
    display: flex;
    gap: 2px;
    padding: 6px 8px 0;
    border-bottom: 1px solid var(--line);
  }
  .tabs button {
    background: none;
    border: 0;
    padding: 7px 9px;
    font-size: 12px;
    color: var(--muted);
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
  }
  .tabs button.on {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .body {
    flex: 1;
    min-height: 0;
    padding: 14px;
  }
</style>
