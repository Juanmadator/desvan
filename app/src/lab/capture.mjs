// Capturas y medidas del laboratorio del treemap. Arranca Vite en este mismo proceso y
// Chromium sin ventana, hace el trabajo y lo cierra todo (también si algo se cuelga).
//
//   node src/lab/capture.mjs [ronda]           capturas en captures/treemap/<ronda>-*.png
//   node src/lab/capture.mjs --perf [--gpu]    mide con el volcado de estrés (~350k rects)
//
// Por defecto usa SwiftShader (no toca la GPU). --gpu usa ANGLE/D3D11 (sesión corta).
// Datos previos (desde la raíz del repo):
//   cargo run --release -p desvan-core --example layout_dump -- app/src/lab/data/lab.bin 1280 840
//   cargo run --release -p desvan-core --example layout_dump -- app/src/lab/data/lab-estres.bin 1280 840 --views 1 --min-area 0.5 --max-rects 3000000 --stress
import { chromium } from "@playwright/test";
import { createServer } from "vite";
import { fileURLToPath } from "node:url";
import { mkdirSync } from "node:fs";
import path from "node:path";

const APP = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const OUT = path.resolve(APP, "../captures/treemap");
const argv = process.argv.slice(2);
const perf = argv.includes("--perf");
const gpu = argv.includes("--gpu");
const round = argv.find((a) => !a.startsWith("--")) ?? "r1";
const PORT = 1430;
const URL = `http://localhost:${PORT}/treemap-lab.html`;
mkdirSync(OUT, { recursive: true });

const server = await createServer({ root: APP, configFile: path.join(APP, "vite.config.ts"), server: { port: PORT, strictPort: true, hmr: false }, logLevel: "error" });
await server.listen();
const args = gpu ? ["--use-angle=d3d11", "--ignore-gpu-blocklist"] : ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"];
const browser = await chromium.launch({ args });
// vigilante: nunca dejar Chromium ni Vite vivos
const watchdog = setTimeout(async () => {
  console.error("[vigilante] tiempo agotado, cerrando");
  await browser.close().catch(() => {});
  await server.close().catch(() => {});
  process.exit(3);
}, 150_000);
watchdog.unref();

const shot = (page, name, opts = {}) => page.screenshot({ path: path.join(OUT, `${round}-${name}.png`), ...opts });
const idle = (page) => page.evaluate(() => window.__lab.idle());

async function captures() {
  const page = await browser.newPage({ viewport: { width: 1640, height: 924 }, deviceScaleFactor: 1 });
  page.on("pageerror", (e) => console.error("[error]", e.message));
  await page.clock.install();
  await page.goto(URL);
  await idle(page);
  await shot(page, "base");

  const center = async (name) => {
    const i = await page.evaluate((n) => window.__lab.find(n), name);
    if (i < 0) throw new Error(`no está ${name}`);
    return page.evaluate((k) => window.__lab.center(k), i);
  };
  // cursor sobre un archivo grande
  let [x, y] = await center("Gustav.pak");
  await page.mouse.move(x - 20, y + 10);
  await idle(page);
  await page.waitForTimeout(250);
  await shot(page, "hover");
  // selección con clic
  [x, y] = await center("pagefile.sys");
  await page.mouse.click(x, y);
  await page.mouse.move(x + 200, y - 60);
  await idle(page);
  await page.waitForTimeout(250);
  await shot(page, "select");
  // resaltado de una categoría (vídeo)
  await page.mouse.move(5, 5);
  await page.evaluate(() => window.__lab.highlight(1));
  await idle(page);
  await shot(page, "highlight");
  await page.evaluate(() => (window.__lab.highlight(null), window.__lab.select(-1)));
  await idle(page);
  // zoom hacia dentro, a mitad y al final (reloj de Playwright para congelar el tiempo)
  await page.clock.pauseAt((await page.evaluate(() => Date.now())) + 1000);
  const users = await page.evaluate(() => window.__lab.find("Users"));
  await page.evaluate((i) => window.__lab.zoomInto(i), users);
  await page.clock.runFor(130);
  await shot(page, "zoom-mid");
  await page.clock.resume();
  await page.waitForTimeout(400);
  await idle(page);
  await shot(page, "zoomed");
  // y hacia fuera
  await page.clock.pauseAt((await page.evaluate(() => Date.now())) + 1000);
  await page.evaluate(() => window.__lab.goUp());
  await page.clock.runFor(60);
  await page.clock.runFor(70);
  await shot(page, "zoomout-mid");
  await page.clock.resume();
  await page.waitForTimeout(400);
  await idle(page);
  await shot(page, "back");
  await page.close();

  // detalle a 2x: bordes y texto
  const p2 = await browser.newPage({ viewport: { width: 1640, height: 924 }, deviceScaleFactor: 2 });
  await p2.goto(URL);
  await idle(p2);
  await shot(p2, "dpr2-crop", { clip: { x: 14, y: 70, width: 640, height: 400 } });
  await p2.close();
  console.log("capturas en", OUT);
}

async function measure() {
  const page = await browser.newPage({ viewport: { width: 1640, height: 924 }, deviceScaleFactor: 1 });
  page.on("pageerror", (e) => console.error("[error]", e.message));
  await page.goto(`${URL}?set=lab-estres`);
  await idle(page);
  const renderer = await page.evaluate(() => {
    const gl = document.createElement("canvas").getContext("webgl2");
    const ext = gl && gl.getExtension("WEBGL_debug_renderer_info");
    return ext ? gl.getParameter(ext.UNMASKED_RENDERER_WEBGL) : "?";
  });
  console.log(renderer);
  console.log(JSON.stringify(await page.evaluate(() => window.__lab.perf(120)), null, 1));
  await page.screenshot({ path: path.join(OUT, `estres-${gpu ? "gpu" : "cpu"}.png`) });
  await page.close();
}

try {
  await (perf ? measure() : captures());
} finally {
  clearTimeout(watchdog);
  await browser.close();
  await server.close();
}
