// Prueba de punta a punta sobre la app real (escritorio oculto + WebView2 por CDP):
// analizar una carpeta temporal, buscar un archivo, enviarlo a la Papelera y deshacerlo.
// Uso: node scripts/e2e.mjs [exe]
import { chromium } from "../app/node_modules/@playwright/test/index.mjs";
import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

const exe = process.argv.find((a) => a.endsWith(".exe")) ?? "target/debug/desvan-app.exe";
const port = 9334;
const root = join(tmpdir(), `desvan-e2e-${Date.now()}`);
mkdirSync(join(root, "fotos", "viaje"), { recursive: true });
mkdirSync(join(root, "juegos"), { recursive: true });
writeFileSync(join(root, "fotos", "viaje", "playa.jpg"), Buffer.alloc(3_000_000, 7));
writeFileSync(join(root, "juegos", "partida-borrable.sav"), Buffer.alloc(5_000_000, 9));
writeFileSync(join(root, "nota.txt"), "hola");
const victim = join(root, "juegos", "partida-borrable.sav");

const fail = (m) => {
  console.error("FALLO:", m);
  process.exitCode = 1;
};

const launcher = spawn("powershell", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "scripts/run-hidden.ps1", "-Exe", exe, "-DebugPort", String(port), "-Desktop", "desvan-e2e"], {
  stdio: ["ignore", "pipe", "inherit"],
});
const pid = await new Promise((resolve, reject) => {
  let buf = "";
  launcher.stdout.on("data", (d) => {
    buf += d;
    const first = buf.split(/\r?\n/)[0].trim();
    if (/^\d+$/.test(first)) resolve(first);
  });
  launcher.on("exit", () => reject(new Error("el lanzador terminó: " + buf)));
});
const cleanup = () => {
  try {
    execFileSync("taskkill", ["/PID", String(pid), "/T", "/F"], { stdio: "ignore" });
  } catch {}
  try {
    rmSync(root, { recursive: true, force: true });
  } catch {}
};
process.on("exit", cleanup);

let browser;
for (let i = 0; i < 160 && !browser; i++) {
  try {
    browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  } catch (e) {
    if (i % 20 === 0) console.log(`intento ${i}: ${String(e.message).slice(0, 160)}`);
    await new Promise((r) => setTimeout(r, 500));
  }
}
if (!browser) throw new Error("sin conexión con el WebView2");
const page = browser.contexts()[0].pages()[0];
page.on("pageerror", (e) => fail(`error en la página: ${e.message}`));
await page.waitForSelector(".home", { timeout: 30000 });

// 1. analizar la carpeta
await page.evaluate((t) => window.__desvanScan(t), root);
await page.waitForSelector(".ex", { timeout: 60000 });
const status = await page.locator(".status").innerText();
if (!/3\s+archivos/.test(status)) fail(`barra de estado inesperada: ${status}`);
console.log("análisis:", status.replace(/\s+/g, " ").trim());

// 2. buscar y seleccionar el archivo
await page.locator(".search input").fill("borrable");
await page.locator(".results .hit").first().click();
await page.waitForFunction(() => document.querySelector(".sel h2")?.textContent?.includes("partida-borrable"), null, { timeout: 5000 });

// 3. Supr → confirmar → desaparece del disco
await page.keyboard.press("Delete");
await page.getByRole("button", { name: "Enviar a la Papelera", exact: true }).click();
await page.waitForSelector(".toast", { timeout: 10000 });
const t1 = await page.locator(".toast").innerText();
if (!t1.includes("Enviado a la Papelera")) fail(`aviso inesperado: ${t1}`);
if (existsSync(victim)) fail("el archivo sigue en el disco");
const after = await page.locator(".status").innerText();
if (!/2\s+archivos/.test(after)) fail(`el total no se actualizó: ${after}`);
console.log("borrado:", t1.replace(/\s+/g, " ").trim());

// 4. Deshacer → vuelve a su sitio y a los totales
await page.locator(".toast .act").click();
await page.waitForFunction(() => document.querySelector(".toast")?.textContent?.includes("Restaurado"), null, { timeout: 10000 });
if (!existsSync(victim)) fail("deshacer no devolvió el archivo");
await page.waitForFunction(() => /3\s+archivos/.test(document.querySelector(".status")?.textContent ?? ""), null, { timeout: 5000 }).catch(() => fail("el total no volvió a 3 archivos"));
console.log("deshacer: archivo restaurado y totales correctos");

await browser.close();
console.log(process.exitCode ? "e2e: CON FALLOS" : "e2e: todo bien");
process.exit(process.exitCode ?? 0);
