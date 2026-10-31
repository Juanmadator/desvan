<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { inTauri, type Volume } from "../lib/api";
  import { bytes, pct } from "../lib/format";
  import Logo from "../components/Logo.svelte";

  const isNtfs = (v: Volume) => v.filesystem.toUpperCase() === "NTFS";

  async function pickFolder() {
    if (!inTauri) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const dir = await open({ directory: true, title: "Elige una carpeta para analizar" });
    if (typeof dir === "string") void app.scan(dir);
  }

  const kindLabel = (v: Volume) => (v.kind === "fija" ? "Disco local" : v.kind === "extraíble" ? "Unidad extraíble" : v.kind === "red" ? "Unidad de red" : v.kind);
</script>

<main class="home">
  <header>
    <Logo size={44} />
    <div>
      <h1>Desván</h1>
      <p class="muted">¿Dónde se ha ido mi espacio?</p>
    </div>
  </header>

  {#if app.volumes.some(isNtfs)}
    <p class="note"><span aria-hidden="true">⚡</span> En los discos NTFS, Desván lee directamente la tabla maestra de archivos (MFT) y analiza el disco entero en segundos. {#if !app.elevated}Windows puede pedirte permiso de administrador: solo se usa para leer.{/if}</p>
  {/if}

  <section class="drives">
    {#each app.volumes as v (v.letter)}
      {@const used = v.total - v.free}
      {@const fast = isNtfs(v)}
      <article class="drive">
        <div class="top">
          <div class="letter">{v.letter}:</div>
          <div class="id">
            <strong>{v.label || kindLabel(v)}</strong>
            <span class="muted">{kindLabel(v)} · {v.filesystem}</span>
          </div>
          {#if fast}<span class="badge fast" title="Lectura directa de la MFT">⚡ MFT</span>{/if}
        </div>
        <div class="bar" aria-label="Ocupado {pct(used / Math.max(1, v.total))}">
          <div class="fill" class:warn={used / v.total > 0.85} style="width:{(used / Math.max(1, v.total)) * 100}%"></div>
        </div>
        <div class="nums">
          <span><strong>{bytes(used)}</strong> ocupados</span>
          <span class="muted">{bytes(v.free)} libres de {bytes(v.total)}</span>
        </div>
        <button class="btn primary go" onclick={() => app.scan(`${v.letter}:`)}>Analizar {v.letter}:</button>
      </article>
    {/each}
    <button class="drive folder" onclick={pickFolder} disabled={!inTauri}>
      <div class="plus">+</div>
      <strong>Analizar una carpeta…</strong>
      <span class="muted">Solo una parte del disco</span>
    </button>
  </section>

  {#if app.error}
    <p class="error">No se pudo analizar: {app.error}</p>
  {/if}

  <footer class="muted">
    Todo ocurre en tu equipo. Desván no envía nada a ningún sitio y nunca borra sin preguntarte: lo que elimines va a la Papelera de reciclaje.
  </footer>
</main>

<style>
  .home {
    height: 100%;
    overflow: auto;
    padding: 48px clamp(20px, 6vw, 80px);
    display: flex;
    flex-direction: column;
    gap: 28px;
    background:
      radial-gradient(900px 500px at 10% -10%, rgba(243, 167, 59, 0.08), transparent 60%),
      radial-gradient(800px 500px at 100% 0%, rgba(79, 209, 197, 0.06), transparent 60%),
      var(--bg);
  }
  header {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  h1 {
    margin: 0;
    font-size: 26px;
    letter-spacing: -0.02em;
  }
  header p {
    margin: 0;
  }
  .note {
    margin: 0;
    max-width: 860px;
    color: var(--muted);
    font-size: 12.5px;
  }
  .note span {
    color: var(--accent);
  }
  .drives {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: 16px;
    max-width: 1200px;
  }
  .drive {
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 16px;
    padding: 18px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    transition: border-color 0.15s, transform 0.15s;
  }
  .drive:hover {
    border-color: var(--line-2);
  }
  .top {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .letter {
    font-size: 30px;
    font-weight: 700;
    letter-spacing: -0.03em;
    width: 54px;
  }
  .id {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }
  .id strong {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .bar {
    height: 8px;
    border-radius: 99px;
    background: var(--raise);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    border-radius: 99px;
    background: linear-gradient(90deg, var(--teal), #3aa7e0);
  }
  .fill.warn {
    background: linear-gradient(90deg, var(--accent), var(--danger));
  }
  .nums {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    flex-wrap: wrap;
  }
  .go {
    align-self: flex-start;
  }
  .folder {
    align-items: center;
    justify-content: center;
    text-align: center;
    border-style: dashed;
    background: transparent;
    min-height: 168px;
    gap: 6px;
  }
  .plus {
    font-size: 28px;
    color: var(--muted);
    line-height: 1;
  }
  .error {
    color: #ffb3c0;
  }
  footer {
    margin-top: auto;
    font-size: 12px;
    max-width: 760px;
  }
</style>
