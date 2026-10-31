<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "./lib/app.svelte";
  import Home from "./screens/Home.svelte";
  import Scanning from "./screens/Scanning.svelte";
  import Explorer from "./screens/Explorer.svelte";

  onMount(() => {
    void app.init();
  });
</script>

{#if app.screen === "inicio"}
  <Home />
{:else if app.screen === "escaneando"}
  <Scanning />
{:else}
  <Explorer />
{/if}

{#if app.toast}
  <div class="toast" role="status">
    <span>{app.toast}</span>
    {#if app.toastAction}
      <button class="act" onclick={() => app.toastAction?.run()}>{app.toastAction.label}</button>
    {/if}
  </div>
{/if}

<style>
  .toast {
    position: fixed;
    bottom: 36px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--raise);
    border: 1px solid var(--line-2);
    border-radius: 10px;
    padding: 9px 16px;
    box-shadow: var(--shadow);
    z-index: 100;
    animation: up 0.2s ease-out;
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .act {
    background: none;
    border: 0;
    color: var(--accent-2);
    font-weight: 600;
    padding: 2px 4px;
  }
  .act:hover {
    text-decoration: underline;
  }
  @keyframes up {
    from {
      transform: translate(-50%, 8px);
      opacity: 0;
    }
  }
</style>
