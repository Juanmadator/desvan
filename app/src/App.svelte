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
  <div class="toast" role="status">{app.toast}</div>
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
  }
  @keyframes up {
    from {
      transform: translate(-50%, 8px);
      opacity: 0;
    }
  }
</style>
