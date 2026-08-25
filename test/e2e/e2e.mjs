// AlkALive end-to-end browser verification.
//
// Verifies the REAL product path in a real (headless) Chromium:
//
//   deploy/index.html → pkg/alkalive_runtime_wasm.js → WASM runtime
//     → .alk compiled at startup by the real compiler
//     → renderer selection logged ("AlkALive renderer selected: …")
//     → render graph executed on the GPU
//     → visible golden-on-black pixels on the canvas
//
// Two runs are performed:
//   1. default flags — WebGPU is attempted first; whichever renderer the
//      runtime selects must render real pixels. The selection line is
//      recorded.
//   2. WebGPU removed from the page before scripts run — the runtime MUST
//      select the WebGL2/GLSL fallback and still render real pixels.
//
// The dev server sets COOP/COEP HTTP response headers (the deployment
// configuration), so `crossOriginIsolated` is also asserted true.
//
// Usage: node e2e.mjs [--headed]

import { chromium } from 'playwright-core';
import { PNG } from 'pngjs';
import { createServer } from 'node:http';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { join, extname, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const DEPLOY_DIR = join(__dirname, '..', '..', 'deploy');
const ARTIFACTS_DIR = join(__dirname, 'artifacts');

const PORT = 8123;
const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript',
  '.mjs': 'text/javascript',
  '.wasm': 'application/wasm',
  '.json': 'application/json',
  '.ts': 'text/plain',
};

function startServer() {
  return new Promise((resolve) => {
    const server = createServer(async (req, res) => {
      // ADR-003 deployment requirement: cross-origin isolation must be
      // enabled via HTTP RESPONSE headers (<meta http-equiv> does not work).
      res.setHeader('Cross-Origin-Opener-Policy', 'same-origin');
      res.setHeader('Cross-Origin-Embedder-Policy', 'require-corp');
      const url = req.url === '/' ? '/index.html' : req.url.split('?')[0];
      if (url === '/favicon.ico') {
        res.statusCode = 204;
        res.end();
        return;
      }
      try {
        const data = await readFile(join(DEPLOY_DIR, url));
        res.setHeader('Content-Type', MIME[extname(url)] ?? 'application/octet-stream');
        res.setHeader('Cache-Control', 'no-store');
        res.end(data);
      } catch {
        res.statusCode = 404;
        res.end('not found');
      }
    });
    server.listen(PORT, () => resolve(server));
  });
}

async function analyzeCanvas(page) {
  const buf = await page.locator('#canvas').screenshot({ type: 'png' });
  const png = PNG.sync.read(buf);
  let black = 0;
  let golden = 0;
  const total = png.width * png.height;
  for (let i = 0; i < png.data.length; i += 4) {
    const r = png.data[i];
    const g = png.data[i + 1];
    const b = png.data[i + 2];
    if (r < 8 && g < 8 && b < 8) black++;
    else if (r > 60 && r > g && g > b) golden++;
  }
  return { width: png.width, height: png.height, total, black, golden };
}

function collectLogs(page, sink) {
  page.on('console', (msg) => sink.push(`[console.${msg.type()}] ${msg.text()}`));
  page.on('pageerror', (err) => sink.push(`[pageerror] ${err.message}`));
}

/**
 * Load the app in a fresh context and capture logs/pixels.
 * @returns {Promise<{logs: string[], isolated: ?boolean, sabOk: ?boolean, pixels: object}>}
 */
async function loadAndCapture(browser, name, { hideWebGPU = false } = {}) {
  const context = await browser.newContext({
    viewport: { width: 800, height: 600 },
    deviceScaleFactor: 1,
  });
  const page = await context.newPage();
  const logs = [];
  collectLogs(page, logs);
  if (hideWebGPU) {
    // Force the fallback path deterministically: remove WebGPU before any
    // page script runs so the runtime's adapter request must fail.
    // `navigator.gpu` lives on the Navigator prototype as an accessor;
    // redefine it there and verify from inside the page.
    await page.addInitScript(() => {
      const proto = Object.getPrototypeOf(navigator);
      try {
        Object.defineProperty(proto, 'gpu', {
          configurable: true,
          get: () => undefined,
        });
      } catch (e) {
        console.warn(`[harness] could not hide navigator.gpu: ${e}`);
      }
      console.log(`[harness] navigator.gpu = ${typeof navigator.gpu}`);
    });
  }

  await page.goto(`http://127.0.0.1:${PORT}/index.html`, { waitUntil: 'load' });
  await page.waitForTimeout(4000);

  let isolated = null;
  let sabOk = null;
  if (!hideWebGPU) {
    isolated = await page.evaluate(() => window.crossOriginIsolated === true);
    sabOk = await page.evaluate(() => {
      try {
        new SharedArrayBuffer(16);
        return true;
      } catch {
        return false;
      }
    });
  }

  const pixels = await analyzeCanvas(page);
  await mkdir(ARTIFACTS_DIR, { recursive: true });
  await writeFile(join(ARTIFACTS_DIR, `${name}.png`), await page.locator('#canvas').screenshot());
  await writeFile(join(ARTIFACTS_DIR, `${name}.log.txt`), logs.join('\n'));
  await context.close();
  return { logs, isolated, sabOk, pixels };
}

function assert(cond, message) {
  if (!cond) throw new Error(`ASSERT FAILED: ${message}`);
}

async function main() {
  const headed = process.argv.includes('--headed');
  const server = await startServer();

  const LAUNCH_ARGS = [
    // Permit real WebGPU where a hardware adapter exists.
    // (--enable-unsafe-swiftshader was evaluated and REJECTED: on Chrome 131
    // headless-shell it disables the automatic software fallback without
    // providing one, leaving neither WebGPU nor WebGL2 available.)
    '--enable-unsafe-webgpu',
  ];

  /**
   * Fresh browser per case: headless GPU-process state does not survive
   * context churn reliably, so each case gets an isolated instance plus a
   * warm-up pass before the real measurement.
   */
  async function withBrowser(fn) {
    const browser = await chromium.launch({ headless: !headed, args: LAUNCH_ARGS });
    try {
      const warm = await browser.newContext({ viewport: { width: 320, height: 240 } });
      const warmPage = await warm.newPage();
      await warmPage.setContent('<canvas id="c"></canvas>');
      await warmPage.evaluate(async () => {
        try {
          if (navigator.gpu) await navigator.gpu.requestAdapter();
          document.getElementById('c').getContext('webgl2');
        } catch {}
      });
      await warmPage.waitForTimeout(1500);
      await warm.close();
      return await fn(browser);
    } finally {
      await browser.close();
    }
  }

  // ---- Run 1: default — attempt WebGPU; either selected path must draw ----
  const def = await withBrowser((b) => loadAndCapture(b, 'default'));

  // ---- Run 2: WebGPU hidden — fallback MUST engage -------------------------
  const nowg = await withBrowser((b) => loadAndCapture(b, 'no-webgpu', { hideWebGPU: true }));

  server.close();

  // ---- Assertions --------------------------------------------------------
  assert(def.pixels.total > 0, 'canvas screenshot captured');
  assert(
    def.pixels.golden > def.pixels.total * 0.0005,
    `golden text visible on default run (got ${def.pixels.golden} golden px of ${def.pixels.total})`
  );
  assert(
    def.logs.some((l) => l.includes('AlkALive renderer selected:')),
    'runtime logged its renderer selection'
  );
  assert(def.isolated === true, 'crossOriginIsolated must be true under COOP/COEP response headers');
  assert(def.sabOk === true, 'SharedArrayBuffer must be constructible when isolated');

  assert(
    nowg.logs.some((l) => l.includes('renderer selected: WebGL2')),
    'fallback run must explicitly select the WebGL2/GLSL renderer'
  );
  assert(
    nowg.pixels.golden > nowg.pixels.total * 0.0005,
    `golden text visible on fallback run (got ${nowg.pixels.golden} golden px of ${nowg.pixels.total})`
  );

  console.log('\n=== AlkALive E2E: ALL ASSERTIONS PASSED ===\n');
  console.log(
    `[default] golden=${def.pixels.golden}/${def.pixels.total}px isolated=${def.isolated}`
  );
  console.log(`[no-webgpu] golden=${nowg.pixels.golden}/${nowg.pixels.total}px`);
  const selectionLine = def.logs.find((l) => l.includes('AlkALive renderer selected:'));
  console.log(`Default-run selection: ${selectionLine}`);
  const fallbackReason = nowg.logs.find((l) => l.includes('wgpu/WGSL renderer unavailable'));
  if (fallbackReason) console.log(`Fallback trigger: ${fallbackReason}`);
}

main().catch((err) => {
  console.error('\n=== AlkALive E2E: FAILURE ===');
  console.error(err.message);
  process.exit(1);
});
