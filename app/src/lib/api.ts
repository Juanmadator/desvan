// API tipada hacia el backend en Rust. Fuera de Tauri (vista previa en el navegador)
// se usa un backend simulado con datos de ejemplo.
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";

export const FLAGS = {
  DIR: 1,
  HIDDEN: 2,
  SYSTEM: 4,
  REPARSE: 8,
  COMPRESSED: 16,
  SPARSE: 32,
  HARDLINK: 64,
  ADS: 128,
  ORPHAN: 256,
  SYNTHETIC: 512,
  METAFILE: 1024,
  OFFLINE: 2048,
  DENIED: 4096,
  DELETED: 8192,
} as const;

export interface Volume {
  letter: string;
  label: string;
  filesystem: string;
  total: number;
  free: number;
  kind: string;
  cluster: number;
}

export interface ScanStats {
  elapsed_ms: number;
  read_ms: number;
  build_ms: number;
  bytes_read: number;
  records: number;
  files: number;
  dirs: number;
  bad_records: number;
  denied: number;
  threads: number;
}

export interface Summary {
  id: number;
  root: number;
  root_path: string;
  method: "mft" | "walk";
  stats: ScanStats;
  volume: { total: number; free: number; cluster: number; filesystem: string; label: string } | null;
  nodes: number;
  tree_bytes: number;
  alloc: number;
  size: number;
  files: number;
  dirs: number;
}

export interface Row {
  id: number;
  name: string;
  alloc: number;
  size: number;
  files: number;
  dirs: number;
  flags: number;
  ext: string;
  cat: number;
  mtime: number;
  kids: number;
}

export interface NodeInfo extends Row {
  path: string;
  parent: number;
  depth: number;
  of_parent: number;
  of_root: number;
}

export interface Hit extends Row {
  path: string;
}

export interface ExtStats {
  exts: { ext: string; cat: number; alloc: number; size: number; files: number }[];
  cats: { cat: number; alloc: number; files: number }[];
}

export interface Progress {
  phase: "starting" | "reading" | "building" | "done";
  done: number;
  total: number;
  files: number;
  bytes: number;
}

export interface JunkHit {
  rule: string;
  title: string;
  description: string;
  risk: "seguro" | "revisar" | "informativo";
  bytes: number;
  nodes: { id: number; path: string; alloc: number }[];
}

export interface DupGroup {
  size: number;
  hash: string;
  wasted: number;
  nodes: { id: number; path: string; mtime: number }[];
}

export interface DupProgress {
  done: number;
  total: number;
  groups: number;
  wasted: number;
  finished: boolean;
}

export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

type Invoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
type Listen = <T>(event: string, cb: (payload: T) => void) => Promise<UnlistenFn>;

let invoke: Invoke = tauriInvoke as Invoke;
let listen: Listen = (event, cb) => tauriListen(event, (e) => cb(e.payload as never));

if (!inTauri) {
  const mock = await import("./mock");
  invoke = mock.invoke as Invoke;
  listen = mock.listen as Listen;
}

export const api = {
  startupArgs: () => invoke<string>("startup_args"),
  volumes: () => invoke<Volume[]>("volumes"),
  elevation: () => invoke<{ elevated: boolean }>("elevation"),
  relaunchAdmin: (target?: string) => invoke<boolean>("relaunch_admin", { target }),
  scanStart: (target: string, preferMft = true) => invoke<number>("scan_start", { target, preferMft }),
  scanCancel: () => invoke<void>("scan_cancel"),
  scanSummary: () => invoke<Summary>("scan_summary"),
  node: (id: number) => invoke<NodeInfo>("node", { id }),
  children: (id: number, offset = 0, limit = 500) => invoke<Row[]>("children", { id, offset, limit }),
  ancestors: (id: number) => invoke<{ id: number; name: string }[]>("ancestors", { id }),
  search: (query: string, limit = 300) => invoke<Hit[]>("search", { query, limit }),
  largest: (id: number, limit = 200) => invoke<Hit[]>("largest", { id, limit }),
  extStats: (id: number) => invoke<ExtStats>("ext_stats", { id }),
  layout: (id: number, width: number, height: number, metric = "alloc", minArea = 4) =>
    invoke<ArrayBuffer>("layout", { id, width, height, metric, minArea }),
  junk: () => invoke<JunkHit[]>("junk"),
  dupesStart: (id: number, minSize = 1 << 20) => invoke<void>("dupes_start", { id, minSize }),
  dupesCancel: () => invoke<void>("dupes_cancel"),
  delete: (ids: number[]) => invoke<{ deleted: number[]; freed: number; root_alloc: number }>("delete", { ids }),
  reveal: (id: number) => invoke<void>("reveal", { id }),
  openPath: (id: number) => invoke<void>("open_path", { id }),
};

export const events = {
  progress: (cb: (p: [number, Progress]) => void) => listen<[number, Progress]>("scan-progress", cb),
  done: (cb: (s: Summary) => void) => listen<Summary>("scan-done", cb),
  error: (cb: (e: [number, string]) => void) => listen<[number, string]>("scan-error", cb),
  dupesProgress: (cb: (p: DupProgress) => void) => listen<DupProgress>("dupes-progress", cb),
  dupesGroup: (cb: (g: DupGroup) => void) => listen<DupGroup>("dupes-group", cb),
};
