// Backend simulado para abrir la interfaz en un navegador normal (sin Tauri).
// Solo cubre la pantalla de inicio; el análisis real necesita la app.
type Cb = (payload: unknown) => void;
const listeners = new Map<string, Cb[]>();

const GB = 1024 ** 3;

export async function invoke<T>(cmd: string, _args?: Record<string, unknown>): Promise<T> {
  switch (cmd) {
    case "volumes":
      return [
        { letter: "C", label: "", filesystem: "NTFS", total: 930 * GB, free: 181 * GB, kind: "fija", cluster: 4096 },
        { letter: "G", label: "Juanmadator", filesystem: "exFAT", total: 1863 * GB, free: 1230 * GB, kind: "fija", cluster: 131072 },
      ] as T;
    case "elevation":
      return { elevated: false } as T;
    case "startup_args":
      return "" as T;
    default:
      throw new Error(`«${cmd}» solo funciona dentro de la app`);
  }
}

export async function listen<T>(event: string, cb: (payload: T) => void) {
  const list = listeners.get(event) ?? [];
  list.push(cb as Cb);
  listeners.set(event, list);
  return () => listeners.set(event, (listeners.get(event) ?? []).filter((x) => x !== cb));
}
