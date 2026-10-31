// Capturas de la app real (Tauri + WebView2) lanzada en un escritorio oculto.
// Uso: node scripts/shots.mjs [exe] [--scan C:] [--out captures/app]
// Conecta Playwright al WebView2 por CDP, espera a la interfaz y guarda capturas de cada pantalla.
import { chromium } from "../app/node_modules/@playwright/test/index.mjs";
import { execFileSync, spawn } from "node:child_process";
import { mkdirSync } from "node:fs";

const args = process.argv.slice(2);
const exe = args.find((a) => a.endsWith(".exe")) ?? "target/debug/desvan-app.exe";
const outIdx = args.indexOf("--out");
const out = outIdx >= 0 ? args[outIdx + 1] : "captures/app";
const scanIdx = args.indexOf("--scan");
const scanTarget = scanIdx >= 0 ? args[scanIdx + 1] : null;
const port = 9333;
const admin = args.includes("--admin");
mkdirSync(out, { recursive: true });
const abs = (p) => (p.includes(":") ? p : `${process.cwd()}/${p}`).replaceAll("/", "\\");

// el lanzador se queda vivo mientras dure la app (mantiene el escritorio oculto)
// con --admin se lanza elevado (en equipos con UAC silencioso no pregunta nada)
if (admin) {
  const inner = `Start-Process powershell -Verb RunAs -WindowStyle Hidden -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','${abs("scripts/run-hidden.ps1")}','-Exe','${abs(exe)}','-DebugPort','${port}'`;
  execFileSync("powershell", ["-NoProfile", "-Command", inner]);
}
const launcher = admin ? null : spawn("powershell", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "scripts/run-hidden.ps1", "-Exe", exe, "-DebugPort", String(port)], {
  stdio: ["ignore", "pipe", "inherit"],
});
const pid = admin ? "elevado" : await new Promise((resolve, reject) => {
  let buf = "";
  launcher.stdout.on("data", (d) => {
    buf += d;
    const first = buf.split(/\r?\n/)[0].trim();
    if (/^\d+$/.test(first)) resolve(first);
  });
  launcher.on("exit", () => reject(new Error("el lanzador terminó: " + buf)));
});
console.log("app pid", pid);
const kill = () => {
  try {
    if (admin) execFileSync("powershell", ["-NoProfile", "-Command", "Start-Process taskkill -Verb RunAs -WindowStyle Hidden -ArgumentList '/IM','desvan-app.exe','/F'"], { stdio: "ignore" });
    else execFileSync("taskkill", ["/PID", String(pid), "/T", "/F"], { stdio: "ignore" });
  } catch {}
};
process.on("exit", kill);

let browser;
for (let i = 0; i < 60; i++) {
  try {
    browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
    break;
  } catch {
    await new Promise((r) => setTimeout(r, 500));
  }
}
if (!browser) throw new Error("no se pudo conectar al WebView2");
const ctx = browser.contexts()[0];
let page = ctx.pages()[0];
for (let i = 0; !page && i < 40; i++) {
  await new Promise((r) => setTimeout(r, 250));
  page = ctx.pages()[0];
}
page.on("console", (m) => m.type() === "error" && console.log("consola:", m.text()));
page.on("pageerror", (e) => console.log("error:", e.message));
await page.waitForSelector(".home, .scan, .ex", { timeout: 30000 });
await page.waitForTimeout(800);
await page.screenshot({ path: `${out}/1-inicio.png` });

if (scanTarget) {
  await page.evaluate((t) => window.__desvanScan?.(t), scanTarget);
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${out}/2-escaneando.png` });
  await page.waitForSelector(".ex", { timeout: 300000 });
  await page.waitForTimeout(1500);
  await page.screenshot({ path: `${out}/3-explorar.png` });
  for (const tab of ["Tipos", "Mayores", "Limpieza"]) {
    await page.getByRole("tab", { name: tab }).click();
    await page.waitForTimeout(1500);
    await page.screenshot({ path: `${out}/4-${tab.toLowerCase()}.png` });
  }
}
await browser.close();
kill();
console.log("capturas en", out);
process.exit(0);
