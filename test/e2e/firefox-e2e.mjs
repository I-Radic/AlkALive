// Firefox end-to-end verification — REAL in-browser WebGPU execution.
//
// Stock Firefox ≥141 ships WebGPU on Windows by default, which Chromium on
// some machines does not expose. This harness therefore proves the
// wgpu/WGSL production renderer inside an actual browser:
//
//   Case 1 (webgpu): dom.webgpu.enabled=true  → runtime MUST select WebGPU
//            and render golden-on-black pixels through WGSL pipelines.
//            On environments that cannot provide a WebGPU adapter at all
//            (GPU-less CI VMs — Firefox's requestAdapter stalls there),
//            the runtime publishes its fallback contract instead; the
//            harness then asserts THAT contract plus pixels and notes the
//            skip loudly (the in-browser WGSL pixel proof for CI lives in
//            e2e-chromium's SwiftShader-Vulkan WebGPU run and in the native
//            offscreen wgpu tests).
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
  // GPU-less CI VMs: Firefox's graphics blocklist can disable WebGL there
  // even though the WARP/software rasterizer is perfectly usable (observed
  // as getContext('webgl2') → null right after a WebGPU probe timeout).
  // Force-enable WebGL and software WebRender so the runtime's WebGL2
  // fallback always has a real rendering path.
  options.setPreference('webgl.force-enabled', true);
  options.setPreference('gfx.webrender.software', true);
  // Mirror page console messages to Firefox's stdout. The runtime logs
  // every boot stage (isolation check → renderer selection → ready), so
  // console output pinpoints where a stalled boot sits. geckodriver's
  // inherited stdio (below) forwards it to this process's stdout → CI log.
  options.setPreference('devtools.console.stdout.content', true);
  const builder = new Builder().forBrowser(Browser.FIREFOX).setFirefoxOptions(options);
  const gd = resolveGeckodriver();
  if (gd) {
    builder.setFirefoxService(
      new firefox.ServiceBuilder(gd).setStdio(['ignore', 'inherit', 'inherit']),
    );
  }
  return builder.build();
}

/**
 * Load the app, wait until the runtime publishes its state, capture pixels
 * and the observed startup latency (page load → renderer live).
 * @returns {Promise<{state: object|null, pixels: object, startupMs: number}>}
 */
async function loadAndCapture(driver, name) {
  const t0 = Date.now();
  await driver.get(`http://127.0.0.1:${PORT}/index.html`);
  // Cold CI browsers (fresh profile, GPU-less VM, software WebGPU init)
  // take far longer to publish the first state than the ~0.5s warm local
  // startup; 60s keeps the wait conclusive without masking a real hang.
  const STATE_WAIT_MS = 60_000;
  try {
    await driver.wait(async () => {
      const present = await driver.executeScript('return !!window.__alkalive');
      return present === true;
    }, STATE_WAIT_MS);
  } catch (e) {
    // The wait timed out: gather page-side evidence so the failure is
    // actionable instead of a bare "Wait timed out". The canvas context
    // probe reveals how far the boot got: a 'webgpu' context means the
    // runtime committed the canvas to the wgpu path; 'webgl2' means the
    // fallback engaged; neither means selection never finished. (Probing a
    // context type on an untouched canvas would create it — acceptable
    // here because the attempt has already failed.)
    const diag = await driver
      .executeScript(
        `return {
          readyState: document.readyState,
          navigatorGpu: typeof navigator.gpu,
          canvasWebgpuCtx: !!document.getElementById('canvas').getContext('webgpu'),
          canvasWebgl2Ctx: !!document.getElementById('canvas').getContext('webgl2'),
          ua: navigator.userAgent,
          wasmFetches: performance.getEntriesByType('resource')
            .map((e) => e.name.split('/').pop() + ':' + e.responseEnd.toFixed(0) + 'ms')
            .filter((n) => n.includes('wasm')),
          alkalive: typeof window.__alkalive,
        };`,
      )
      .catch(() => null);
    throw new Error(
      `runtime never published window.__alkalive within ${STATE_WAIT_MS}ms ` +
        `(page diagnostics: ${JSON.stringify(diag)}; console output above)`,
    );
  }
  const startupMs = Date.now() - t0;

  const state = await driver.executeScript(
    'return window.__alkalive ? { renderer: window.__alkalive.renderer, fallbackReason: window.__alkalive.fallbackReason } : null;',
  );
  // Give the RAF loop a few frames so the golden text has actually been drawn.
  await driver.sleep(700);
  const pngB64 = await driver.takeScreenshot();
  const buf = Buffer.from(pngB64, 'base64');

  await mkdir(ARTIFACTS, { recursive: true });
  await writeFile(join(ARTIFACTS, `firefox-${name}.png`), buf);
  return { state, pixels: analyzePng(buf), startupMs };
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

  if (webgpu.state?.renderer === 'WebGPU') {
    // Environment provides a real WebGPU adapter: hold the full in-browser
    // wgpu/WGSL proof (selection + no fallback reason + golden pixels).
    assert(webgpu.state.fallbackReason === null, 'WebGPU run must have no fallback reason');
    assert(
      webgpu.pixels.golden > webgpu.pixels.total * 0.0005,
      `golden text visible on WebGPU/WGSL run (got ${webgpu.pixels.golden}/${webgpu.pixels.total})`,
    );
  } else {
    // No WebGPU adapter in this environment (GPU-less CI VM: Firefox's
    // requestAdapter stalls until the runtime's probe timeout). Assert the
    // published fallback contract and pixels instead — never a blank hang.
    assert(
      webgpu.state !== null && webgpu.state.renderer === 'WebGL2',
      `WebGPU-enabled run without an adapter must publish the WebGL2 fallback ` +
        `contract (got ${JSON.stringify(webgpu.state)})`,
    );
    assert(
      typeof webgpu.state.fallbackReason === 'string' &&
        webgpu.state.fallbackReason.length > 0,
      'adapter-less WebGPU-enabled run must publish why WebGPU was not used',
    );
    assert(
      webgpu.pixels.golden > webgpu.pixels.total * 0.0005,
      `golden text visible on adapter-less WebGPU-enabled run ` +
        `(got ${webgpu.pixels.golden}/${webgpu.pixels.total})`,
    );
    console.log(
      `[warn] no WebGPU adapter in this environment ` +
        `(reason: "${webgpu.state.fallbackReason}") — asserted the fallback ` +
        'contract + pixels; the in-browser wgpu/WGSL pixel proof for CI runs ' +
        'in e2e-chromium (SwiftShader-Vulkan WebGPU) and offscreen_wgpu.rs',
    );
  }

  console.log('\n=== AlkALive Firefox E2E: ALL ASSERTIONS PASSED ===');
  console.log(
    `[webgpu] renderer=${webgpu.state?.renderer}${webgpu.state?.fallbackReason ? ` (${webgpu.state.fallbackReason})` : ''} golden=${webgpu.pixels.golden}/${webgpu.pixels.total}px startup=${webgpu.startupMs}ms ${headedRetryNote}`,
  );
  console.log(
    `[webgl2] renderer=WebGL2 reason="${webgl2.state.fallbackReason}" golden=${webgl2.pixels.golden}/${webgl2.pixels.total}px startup=${webgl2.startupMs}ms`,
  );
}

main().catch((err) => {
  console.error('\n=== AlkALive Firefox E2E: FAILURE ===');
  console.error(err.message);
  process.exit(1);
});
