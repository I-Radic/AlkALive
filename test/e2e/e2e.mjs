// AlkALive end-to-end browser verification (Chromium via Playwright).
//
// Verifies the REAL product path in a real (headless) browser:
//
//   deploy/index.html → pkg/alkalive_runtime_wasm.js → WASM runtime
//     → .alk compiled at startup by the real compiler (compile_full:
//       schedule → dep-graph → e-graph)
//     → renderer selection logged AND published to window.__alkalive
//     → render graph executed on the GPU
//     → visible golden-on-black pixels on the canvas
//
// Runs:
//   1. default flags — WebGPU attempted first; whichever renderer is
//      selected must render real pixels. Selection asserted via
//      window.__alkalive + console logs.
//   2. WebGPU removed before scripts run — runtime MUST select WebGL2/GLSL,
//      publish a fallback reason, and still render real pixels.
//
// NOTE on WebGPU-in-browser proof: where Chromium lacks an adapter, this
// harness still passes by asserting the *selection contract*; the actual
// wgpu/WGSL rendering path is proven IN-BROWSER by firefox-e2e.mjs
// (Firefox ≥141 ships WebGPU) and OFFSCREEN by offscreen_wgpu.rs.
//
// Usage: node e2e.mjs [--headed]

import { chromium } from 'playwright-core';
import { writeFile, mkdir } from 'node:fs/promises';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { startServer, analyzePng, assert } from './harness.mjs';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ARTIFACTS_DIR = join(__dirname, 'artifacts');

const PORT = 8123;

function collectLogs(page, sink) {
  page.on('console', (msg) => sink.push(`[console.${msg.type()}] ${msg.text()}`));
  page.on('pageerror', (err) => sink.push(`[pageerror] ${err.message}`));
}

/**
 * Load the app in a fresh context and capture logs/state/pixels.
 * @returns {Promise<{logs: string[], state: ?object, isolated: ?boolean, sabOk: ?boolean, pixels: object}>}
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

  const state = await page.evaluate(() =>
    window.__alkalive
      ? { renderer: window.__alkalive.renderer, fallbackReason: window.__alkalive.fallbackReason }
      : null,
  );

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

  const buf = await page.locator('#canvas').screenshot({ type: 'png' });
  const pixels = analyzePng(buf);
  await mkdir(ARTIFACTS_DIR, { recursive: true });
  await writeFile(join(ARTIFACTS_DIR, `${name}.png`), buf);
  await writeFile(join(ARTIFACTS_DIR, `${name}.log.txt`), logs.join('\n'));
  await context.close();
  return { logs, state, isolated, sabOk, pixels };
}

async function main() {
  const headed = process.argv.includes('--headed');
  const server = await startServer(PORT);

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

  // ---- Assertions ----------------------------------------------------------
  assert(def.pixels.total > 0, 'canvas screenshot captured');
  assert(def.state !== null, 'runtime must publish window.__alkalive');
  assert(
    ['WebGPU', 'WebGL2'].includes(def.state.renderer),
    `published renderer must be a known path (got ${JSON.stringify(def.state)})`,
  );
  assert(
    def.pixels.golden > def.pixels.total * 0.0005,
    `golden text visible on default run (got ${def.pixels.golden} golden px of ${def.pixels.total})`,
  );
  assert(
    def.logs.some((l) => l.includes('AlkALive renderer selected:')),
    'runtime logged its renderer selection',
  );
  assert(def.isolated === true, 'crossOriginIsolated must be true under COOP/COEP response headers');
  assert(def.sabOk === true, 'SharedArrayBuffer must be constructible when isolated');

  assert(
    nowg.state !== null && nowg.state.renderer === 'WebGL2',
    `fallback run must publish WebGL2 selection (got ${JSON.stringify(nowg.state)})`,
  );
  assert(
    typeof nowg.state.fallbackReason === 'string' && nowg.state.fallbackReason.length > 0,
    'fallback run must publish a fallback reason',
  );
  assert(
    nowg.logs.some((l) => l.includes('renderer selected: WebGL2')),
    'fallback run must explicitly select the WebGL2/GLSL renderer',
  );
  assert(
    nowg.pixels.golden > nowg.pixels.total * 0.0005,
    `golden text visible on fallback run (got ${nowg.pixels.golden} golden px of ${nowg.pixels.total})`,
  );

  console.log('\n=== AlkALive E2E: ALL ASSERTIONS PASSED ===\n');
  console.log(
    `[default]   renderer=${def.state.renderer}${def.state.fallbackReason ? ` (${def.state.fallbackReason})` : ''} golden=${def.pixels.golden}/${def.pixels.total}px isolated=${def.isolated}`,
  );
  console.log(
    `[no-webgpu] renderer=${nowg.state.renderer} reason="${nowg.state.fallbackReason}" golden=${nowg.pixels.golden}/${nowg.pixels.total}px`,
  );
}

main().catch((err) => {
  console.error('\n=== AlkALive E2E: FAILURE ===');
  console.error(err.message);
  process.exit(1);
});
