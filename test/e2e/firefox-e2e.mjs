// Firefox end-to-end verification — REAL in-browser WebGPU execution.
//
// Stock Firefox ≥141 ships WebGPU on Windows by default, which Chromium on
// some machines does not expose. This harness therefore proves the
// wgpu/WGSL production renderer inside an actual browser:
//
//   Case 1 (webgpu): dom.webgpu.enabled=true  → runtime MUST select WebGPU
//            and render golden-on-black pixels through WGSL pipelines.
//   Case 2 (webgl2): dom.webgpu.enabled=false → runtime MUST fall back to
//            WebGL2/GLSL, publish the reason, and still render.
//
// The active path is asserted via `window.__alkalive`, published by the
// runtime itself at selection time.
//
// Usage: node firefox-e2e.mjs [--headed]
//   Headless is attempted first; when Firefox's GPU process cannot create an
//   adapter without a window session (platform-dependent), Case 1
//   automatically retries headed so the proof never silently degrades.

import { Builder, Browser } from 'selenium-webdriver';
import firefox from 'selenium-webdriver/firefox.js';
import { writeFile, mkdir } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { startServer, analyzePng, assert } from './harness.mjs';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ARTIFACTS = join(__dirname, 'artifacts');
const PORT = 8124;

/** Locate geckodriver: env override → pinned artifacts dir → PATH. */
function resolveGeckodriver() {
  const exe = process.platform === 'win32' ? 'geckodriver.exe' : 'geckodriver';
  if (process.env.GECKODRIVER && existsSync(process.env.GECKODRIVER)) {
    return process.env.GECKODRIVER;
  }
  const pinned = join(ARTIFACTS, 'geckodriver', exe);
  if (existsSync(pinned)) return pinned;
  return null; // selenium falls back to PATH lookup
}

/** Build a WebDriver instance for Firefox with a given WebGPU preference. */
async function makeDriver(webGpuEnabled, headed) {
  const options = new firefox.Options();
  if (!headed) options.addArguments('-headless');
  options.setPreference('dom.webgpu.enabled', webGpuEnabled);
  const builder = new Builder().forBrowser(Browser.FIREFOX).setFirefoxOptions(options);
  const gd = resolveGeckodriver();
  if (gd) builder.setFirefoxService(new firefox.ServiceBuilder(gd));
  return builder.build();
}

/**
 * Load the app, wait for startup, publish state, capture pixels.
 * @returns {Promise<{state: object|null, pixels: object}>}
 */
async function loadAndCapture(driver, name) {
  await driver.get(`http://127.0.0.1:${PORT}/index.html`);
  await driver.sleep(4000);

  const state = await driver.executeScript(
    'return window.__alkalive ? { renderer: window.__alkalive.renderer, fallbackReason: window.__alkalive.fallbackReason } : null;',
  );
  const pngB64 = await driver.takeScreenshot();
  const buf = Buffer.from(pngB64, 'base64');

  await mkdir(ARTIFACTS, { recursive: true });
  await writeFile(join(ARTIFACTS, `firefox-${name}.png`), buf);
  return { state, pixels: analyzePng(buf) };
}

async function main() {
  const forceHeaded = process.argv.includes('--headed');
  const server = await startServer(PORT);

  // ---- Case 1: WebGPU enabled --------------------------------------------
  let webgpu;
  let headedRetryNote = '';
  let d1;
  try {
    d1 = await makeDriver(true, forceHeaded);
    webgpu = await loadAndCapture(d1, 'webgpu');
  } finally {
    if (d1) await d1.quit();
  }
  if (webgpu.state?.renderer !== 'WebGPU' && !forceHeaded) {
    console.log(
      `[info] headless run selected "${webgpu.state?.renderer ?? 'nothing'}"; ` +
        'retrying headed — Firefox needs its GPU process for adapter creation',
    );
    let d1h;
    try {
      d1h = await makeDriver(true, true);
      webgpu = await loadAndCapture(d1h, 'webgpu-headed');
      headedRetryNote = '(headed retry: no headless adapter)';
    } finally {
      if (d1h) await d1h.quit();
    }
  }

  // ---- Case 2: WebGPU disabled by pref → forced fallback ------------------
  let webgl2;
  let d2;
  try {
    d2 = await makeDriver(false, true);
    webgl2 = await loadAndCapture(d2, 'webgl2');
  } finally {
    if (d2) await d2.quit();
  }

  server.close();

  // ---- Assertions ---------------------------------------------------------
  assert(
    webgl2.state !== null,
    'runtime must publish window.__alkalive',
  );
  assert(
    webgl2.state.renderer === 'WebGL2',
    `WebGPU-disabled run must select WebGL2 (got ${JSON.stringify(webgl2.state)})`,
  );
  assert(
    typeof webgl2.state.fallbackReason === 'string' &&
      webgl2.state.fallbackReason.length > 0,
    'fallback run must publish the fallback reason',
  );
  assert(
    webgl2.pixels.golden > webgl2.pixels.total * 0.0005,
    `golden text visible on WebGL2 run (got ${webgl2.pixels.golden}/${webgl2.pixels.total})`,
  );

  assert(
    webgpu.state !== null && webgpu.state.renderer === 'WebGPU',
    `WebGPU-enabled run must select the wgpu/WGSL renderer IN-BROWSER ` +
      `(got ${JSON.stringify(webgpu.state)}). If no adapter exists here, run on ` +
      'a machine/browser with WebGPU (Firefox ≥141 Windows/macOS, or Chrome ≥113).',
  );
  assert(webgpu.state.fallbackReason === null, 'WebGPU run must have no fallback reason');
  assert(
    webgpu.pixels.golden > webgpu.pixels.total * 0.0005,
    `golden text visible on WebGPU/WGSL run (got ${webgpu.pixels.golden}/${webgpu.pixels.total})`,
  );

  console.log('\n=== AlkALive Firefox E2E: ALL ASSERTIONS PASSED ===');
  console.log(
    `[webgpu] renderer=WebGPU golden=${webgpu.pixels.golden}/${webgpu.pixels.total}px ${headedRetryNote}`,
  );
  console.log(
    `[webgl2] renderer=WebGL2 reason="${webgl2.state.fallbackReason}" golden=${webgl2.pixels.golden}/${webgl2.pixels.total}px`,
  );
}

main().catch((err) => {
  console.error('\n=== AlkALive Firefox E2E: FAILURE ===');
  console.error(err.message);
  process.exit(1);
});
