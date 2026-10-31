// Estado global de la app (Svelte 5 runes).
import { api, events, type NodeInfo, type Progress, type Summary, type Volume } from "./api";

export type Screen = "inicio" | "escaneando" | "explorar";
export type Metric = "alloc" | "size";

class AppStore {
  screen = $state<Screen>("inicio");
  volumes = $state<Volume[]>([]);
  elevated = $state(false);
  /** Lo que se está escaneando ("C:" o una carpeta) */
  target = $state("");
  scanId = 0;
  progress = $state<Progress | null>(null);
  startedAt = $state(0);
  summary = $state<Summary | null>(null);
  /** Nodo que llena el mapa (zoom) */
  focus = $state(0);
  selected = $state<number | null>(null);
  selectedInfo = $state<NodeInfo | null>(null);
  metric = $state<Metric>("alloc");
  /** Cambia tras borrar: las vistas vuelven a pedir datos */
  version = $state(0);
  error = $state<string | null>(null);
  toast = $state<string | null>(null);
  /** Categoría resaltada en el mapa (desde la pestaña Tipos) */
  highlightCat = $state<number | null>(null);
  /** Pestaña del panel derecho */
  tab = $state<"seleccion" | "tipos" | "mayores" | "limpieza" | "duplicados">("seleccion");
  /** Petición de borrado pendiente de confirmar */
  pendingDelete = $state<{ ids: number[]; items: { name: string; path: string; alloc: number }[] } | null>(null);
  private toastTimer = 0;

  async init() {
    await events.progress(([id, p]) => {
      if (id === this.scanId) this.progress = p;
    });
    await events.done((s) => {
      if (s.id !== this.scanId) return;
      this.summary = s;
      this.focus = s.root;
      this.selected = null;
      this.selectedInfo = null;
      this.screen = "explorar";
    });
    await events.error(([id, e]) => {
      if (id !== this.scanId) return;
      this.screen = "inicio";
      if (e !== "cancelado") this.error = e;
    });
    await this.refreshVolumes();
    this.elevated = (await api.elevation()).elevated;
    // gancho para las pruebas automáticas
    (window as unknown as { __desvanScan?: (t: string) => Promise<void> }).__desvanScan = (t) => this.scan(t);
    // arranque tras "reiniciar como administrador": --scan C:
    const m = /--scan\s+(\S+)/.exec(await api.startupArgs());
    if (m) void this.scan(m[1]);
  }

  async refreshVolumes() {
    this.volumes = await api.volumes();
  }

  async scan(target: string) {
    this.error = null;
    this.target = target;
    this.progress = null;
    this.startedAt = performance.now();
    this.screen = "escaneando";
    try {
      this.scanId = await api.scanStart(target, true);
    } catch (e) {
      this.screen = "inicio";
      this.error = String(e);
    }
  }

  async cancel() {
    await api.scanCancel();
  }

  async select(id: number | null) {
    this.selected = id;
    this.selectedInfo = id === null ? null : await api.node(id);
  }

  async zoom(id: number) {
    this.focus = id;
  }

  async zoomOut() {
    if (!this.summary || this.focus === this.summary.root) return;
    const info = await api.node(this.focus);
    if (info.parent !== 0xffffffff) this.focus = info.parent;
  }

  /** Envía a la Papelera y actualiza las vistas sin volver a escanear. */
  async remove(ids: number[]) {
    const r = await api.delete(ids);
    if (this.summary) this.summary = { ...this.summary, alloc: r.root_alloc };
    if (this.selected !== null && ids.includes(this.selected)) {
      this.selected = null;
      this.selectedInfo = null;
    }
    this.version++;
    return r;
  }

  openFile = async (id: number) => {
    try {
      await api.openPath(id);
    } catch (e) {
      this.notify(`No se pudo abrir: ${e}`);
    }
  };

  async reveal(id: number) {
    try {
      await api.reveal(id);
    } catch (e) {
      this.notify(`No se pudo mostrar: ${e}`);
    }
  }

  async copyPath(id: number) {
    const info = await api.node(id);
    await navigator.clipboard.writeText(info.path);
    this.notify("Ruta copiada");
  }

  /** Pide confirmación antes de mandar a la Papelera. */
  async askDelete(ids: number[]) {
    const infos = await Promise.all(ids.map((i) => api.node(i)));
    this.pendingDelete = { ids, items: infos.map((n) => ({ name: n.name, path: n.path, alloc: n.alloc })) };
  }

  /** Muestra un nodo: lo selecciona y, si queda fuera del mapa actual, amplía su carpeta. */
  async show(id: number) {
    const chain = await api.ancestors(id);
    if (!chain.some((c) => c.id === this.focus)) {
      const parent = chain.length >= 2 ? chain[chain.length - 2].id : id;
      this.focus = parent;
    }
    await this.select(id);
  }

  notify(msg: string) {
    this.toast = msg;
    clearTimeout(this.toastTimer);
    this.toastTimer = window.setTimeout(() => (this.toast = null), 3500);
  }

  backHome() {
    this.screen = "inicio";
    void this.refreshVolumes();
  }
}

export const app = new AppStore();
