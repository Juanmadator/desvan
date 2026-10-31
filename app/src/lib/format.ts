// Formatos en español: tamaños, números, porcentajes y fechas.

const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

/** 1536 → "1,5 KB" (base 1024). */
export function bytes(n: number, digits?: number): string {
  if (!Number.isFinite(n) || n < 0) return "—";
  if (n < 1024) return `${n} B`;
  let v = n;
  let i = 0;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  const d = digits ?? (v >= 100 ? 0 : v >= 10 ? 1 : 2);
  return `${v.toLocaleString("es-ES", { minimumFractionDigits: d, maximumFractionDigits: d })} ${UNITS[i]}`;
}

/** Tamaño separado en cifra y unidad, para tipografía grande. */
export function bytesParts(n: number): [string, string] {
  const s = bytes(n);
  const i = s.lastIndexOf(" ");
  return [s.slice(0, i), s.slice(i + 1)];
}

export function count(n: number): string {
  return n.toLocaleString("es-ES");
}

export function pct(f: number, digits = 1): string {
  if (!Number.isFinite(f)) return "—";
  const v = f * 100;
  if (v > 0 && v < 0.1) return "<0,1 %";
  return `${v.toLocaleString("es-ES", { maximumFractionDigits: digits, minimumFractionDigits: v < 10 ? digits : 0 })} %`;
}

export function duration(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)} ms`;
  const s = ms / 1000;
  if (s < 60) return `${s.toLocaleString("es-ES", { maximumFractionDigits: s < 10 ? 2 : 1 })} s`;
  const m = Math.floor(s / 60);
  return `${m} min ${Math.round(s % 60)} s`;
}

const rtf = new Intl.RelativeTimeFormat("es", { numeric: "auto" });

/** Fecha relativa: "hace 3 meses". `unix` en segundos. */
export function ago(unix: number): string {
  if (!unix) return "—";
  const diff = unix - Date.now() / 1000;
  const abs = Math.abs(diff);
  const table: [number, Intl.RelativeTimeFormatUnit][] = [
    [60, "second"],
    [3600, "minute"],
    [86400, "hour"],
    [86400 * 30, "day"],
    [86400 * 365, "month"],
    [Infinity, "year"],
  ];
  const div = [1, 60, 3600, 86400, 86400 * 30, 86400 * 365];
  for (let i = 0; i < table.length; i++) {
    if (abs < table[i][0]) return rtf.format(Math.round(diff / div[i]), table[i][1]);
  }
  return "—";
}

export function date(unix: number): string {
  if (!unix) return "—";
  return new Date(unix * 1000).toLocaleDateString("es-ES", { day: "numeric", month: "short", year: "numeric" });
}
