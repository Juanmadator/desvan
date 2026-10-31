<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "../lib/app.svelte";
  import { bytes, count, duration } from "../lib/format";
  import Logo from "../components/Logo.svelte";

  let now = $state(performance.now());
  onMount(() => {
    let raf = 0;
    const tick = () => {
      now = performance.now();
      raf = requestAnimationFrame(tick);
    };
    tick();
    return () => cancelAnimationFrame(raf);
  });

  const p = $derived(app.progress);
  const elapsed = $derived(now - app.startedAt);
  const mft = $derived((p?.bytes ?? 0) > 0);
  const frac = $derived(p && p.total > 0 ? Math.min(1, p.done / p.total) : 0);
  const rate = $derived(elapsed > 300 && p ? (p.files / elapsed) * 1000 : 0);
  const phase = $derived.by(() => {
    if (!p || p.phase === "starting") return "Preparando…";
    if (p.phase === "reading") return mft ? "Leyendo la tabla maestra de archivos (MFT)…" : "Recorriendo carpetas…";
    if (p.phase === "building") return "Ordenando el árbol…";
    return "Terminando…";
  });
</script>

<main class="scan">
  <div class="card">
    <div class="logo"><Logo size={56} /></div>
    <p class="target">Analizando <strong>{app.target}</strong></p>
    <div class="big">{count(p?.files ?? 0)}</div>
    <p class="label muted">archivos encontrados</p>

    <div class="bar" class:indeterminate={!p || p.total <= 1}>
      <div class="fill" style="width:{frac * 100}%"></div>
    </div>
    <p class="phase">{phase}</p>

    <div class="stats">
      <div><strong>{duration(elapsed)}</strong><span>tiempo</span></div>
      {#if mft}
        <div><strong>{bytes(p?.bytes ?? 0)}</strong><span>de MFT leídos</span></div>
      {:else}
        <div><strong>{count(p?.done ?? 0)}</strong><span>carpetas</span></div>
      {/if}
      <div><strong>{count(Math.round(rate))}</strong><span>archivos/s</span></div>
    </div>

    <button class="btn" onclick={() => app.cancel()}>Cancelar</button>
  </div>
</main>

<style>
  .scan {
    height: 100%;
    display: grid;
    place-items: center;
    background: radial-gradient(700px 400px at 50% 30%, rgba(243, 167, 59, 0.09), transparent 70%), var(--bg);
  }
  .card {
    width: min(520px, 92vw);
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 10px;
  }
  .logo {
    animation: pulse 1.6s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      transform: scale(0.94);
      opacity: 0.8;
    }
  }
  .target {
    margin: 8px 0 0;
    color: var(--muted);
  }
  .target strong {
    color: var(--text);
  }
  .big {
    font-size: 64px;
    font-weight: 700;
    letter-spacing: -0.04em;
    line-height: 1;
    margin-top: 10px;
  }
  .label {
    margin: 0;
  }
  .bar {
    width: 100%;
    height: 6px;
    border-radius: 99px;
    background: var(--raise);
    overflow: hidden;
    margin-top: 18px;
    position: relative;
  }
  .fill {
    height: 100%;
    background: linear-gradient(90deg, var(--accent), var(--accent-2));
    border-radius: 99px;
    transition: width 0.15s linear;
  }
  .bar.indeterminate .fill {
    width: 30% !important;
    position: absolute;
    animation: slide 1.1s ease-in-out infinite;
  }
  @keyframes slide {
    from {
      left: -30%;
    }
    to {
      left: 100%;
    }
  }
  .phase {
    margin: 0;
    color: var(--muted);
    min-height: 20px;
  }
  .stats {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 10px;
    width: 100%;
    margin: 14px 0 18px;
  }
  .stats div {
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 10px;
    display: flex;
    flex-direction: column;
  }
  .stats strong {
    font-size: 17px;
  }
  .stats span {
    font-size: 11px;
    color: var(--muted);
  }
</style>
