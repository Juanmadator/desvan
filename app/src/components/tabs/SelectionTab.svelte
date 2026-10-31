<script lang="ts">
  import { app } from "../../lib/app.svelte";
  import { api, FLAGS, type NodeInfo } from "../../lib/api";
  import { ago, bytes, bytesParts, count, date, pct } from "../../lib/format";
  import { catColor, catLabel } from "../../lib/categories";

  // sin selección se muestra la carpeta del mapa
  let info = $state<NodeInfo | null>(null);
  $effect(() => {
    void app.version;
    const id = app.selected ?? app.focus;
    if (app.summary) void api.node(id).then((n) => (info = n));
  });

  const badges = $derived.by(() => {
    if (!info) return [];
    const f = info.flags;
    const b: [string, string][] = [];
    if (f & FLAGS.HIDDEN) b.push(["Oculto", ""]);
    if (f & FLAGS.SYSTEM) b.push(["Sistema", ""]);
    if (f & FLAGS.COMPRESSED) b.push(["Comprimido", "Comprimido por NTFS: ocupa menos en disco"]);
    if (f & FLAGS.SPARSE) b.push(["Disperso", "Solo ocupan disco las partes con datos"]);
    if (f & FLAGS.HARDLINK) b.push(["Enlace duro", "Comparte datos con otro nombre: no suma en los totales"]);
    if (f & FLAGS.ADS) b.push(["Flujos alternativos", "Tiene flujos de datos alternativos (ADS)"]);
    if (f & FLAGS.OFFLINE) b.push(["En la nube", "Marcador de OneDrive u otro servicio: no está descargado"]);
    if (f & FLAGS.REPARSE) b.push(["Enlace", "Unión o enlace simbólico: su contenido está en otra parte"]);
    if (f & FLAGS.METAFILE) b.push(["Metadatos NTFS", "Estructura interna del sistema de archivos"]);
    if (f & FLAGS.DENIED) b.push(["Sin permiso", "No se pudo leer su contenido"]);
    return b;
  });
  const isDir = $derived(info ? (info.flags & FLAGS.DIR) !== 0 : false);
  const value = $derived(info ? (app.metric === "alloc" ? info.alloc : info.size) : 0);
  const canDelete = $derived(info && !(info.flags & (FLAGS.SYNTHETIC | FLAGS.METAFILE)) && info.id !== app.summary?.root);
</script>

{#if info}
  <section class="sel">
    <div class="kind">
      <span class="sw" style="background:{isDir ? '#c9973f' : catColor(info.cat)}"></span>
      {isDir ? "Carpeta" : `${catLabel(info.cat)}${info.ext ? ` · .${info.ext}` : ""}`}
    </div>
    <h2 title={info.name}>{info.name}</h2>
    <p class="path" title={info.path}>{info.path}</p>

    <div class="big">
      <span class="n">{bytesParts(value)[0]}</span><span class="u">{bytesParts(value)[1]}</span>
    </div>
    <p class="muted sub">
      {#if app.metric === "alloc"}{bytes(info.size)} de datos{:else}{bytes(info.alloc)} en disco{/if}
      {#if info.size > 0 && info.alloc > 0 && Math.abs(info.alloc - info.size) / info.size > 0.25}
        · {info.alloc > info.size ? "mucho espacio desaprovechado en clústeres" : "ahorro por compresión o dispersión"}
      {/if}
    </p>

    <div class="meters">
      <div class="meter">
        <span>De su carpeta</span>
        <div class="track"><div style="width:{info.of_parent * 100}%"></div></div>
        <strong>{pct(info.of_parent)}</strong>
      </div>
      <div class="meter">
        <span>Del total analizado</span>
        <div class="track"><div style="width:{info.of_root * 100}%"></div></div>
        <strong>{pct(info.of_root)}</strong>
      </div>
    </div>

    <dl>
      {#if isDir}
        <dt>Archivos</dt>
        <dd>{count(info.files)}</dd>
        <dt>Subcarpetas</dt>
        <dd>{count(info.dirs)}</dd>
      {/if}
      <dt>Modificado</dt>
      <dd title={date(info.mtime)}>{ago(info.mtime)}</dd>
    </dl>

    {#if badges.length}
      <div class="badges">
        {#each badges as [label, tip]}<span class="badge" title={tip}>{label}</span>{/each}
      </div>
    {/if}

    <div class="acts">
      {#if isDir && app.focus !== info.id}
        <button class="btn sm" onclick={() => app.zoom(info!.id)}>Ampliar</button>
      {/if}
      {#if !isDir}
        <button class="btn sm" onclick={() => app.openFile(info!.id)}>Abrir</button>
      {/if}
      <button class="btn sm" onclick={() => app.reveal(info!.id)}>Mostrar en el Explorador</button>
      <button class="btn sm ghost" onclick={() => app.copyPath(info!.id)}>Copiar ruta</button>
      {#if canDelete}
        <button class="btn sm danger" onclick={() => app.askDelete([info!.id])}>Enviar a la Papelera…</button>
      {/if}
    </div>
  </section>
{/if}

<style>
  .sel {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .kind {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }
  .sw {
    width: 10px;
    height: 10px;
    border-radius: 3px;
  }
  h2 {
    margin: 0;
    font-size: 18px;
    letter-spacing: -0.01em;
    word-break: break-word;
  }
  .path {
    margin: 0;
    color: var(--faint);
    font-size: 12px;
    word-break: break-all;
    user-select: text;
  }
  .big {
    margin-top: 8px;
    display: flex;
    align-items: baseline;
    gap: 6px;
  }
  .big .n {
    font-size: 40px;
    font-weight: 700;
    letter-spacing: -0.03em;
    line-height: 1;
  }
  .big .u {
    font-size: 18px;
    color: var(--muted);
    font-weight: 600;
  }
  .sub {
    margin: 0;
    font-size: 12px;
  }
  .meters {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 6px 0;
  }
  .meter {
    display: grid;
    grid-template-columns: 120px 1fr 56px;
    gap: 8px;
    align-items: center;
    font-size: 12px;
  }
  .meter span {
    color: var(--muted);
  }
  .meter strong {
    text-align: right;
  }
  .track {
    height: 6px;
    border-radius: 3px;
    background: var(--raise);
    overflow: hidden;
  }
  .track div {
    height: 100%;
    background: linear-gradient(90deg, var(--accent), var(--accent-2));
    border-radius: 3px;
  }
  dl {
    display: grid;
    grid-template-columns: 110px 1fr;
    gap: 4px 8px;
    margin: 4px 0;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
  }
  .badges {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
  }
  .acts {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 8px;
  }
</style>
