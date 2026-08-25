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
//      runtime selects must render real pixels. The selection line and the
//      attempted path are recorded.
//   2. WebGPU disabled — the runtime MUST select the WebGL2/GLSL fallback
//      and still render real pixels.
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
      try {
        const data = await readFile(join(DEPLOY_DIR, url));
        res.setHeader('Content-Type', MIME[extname(url)] ?? 'application/octet-stream');
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

async function runCase(browser, name, flags) {
  const logs = [];
  const context = await browser.newContext({
    viewport: { width: 800, height: 600 },
    deviceScaleFactor: 1,
  });
  const page = await context.newPage();
  collectLogs(page, logs);

  await page.goto(`http://127.0.0.1:${PORT}/index.html`, { waitUntil: 'load' });
  // Give the runtime time to initialize + render a few frames.
  await page.waitForSelector('#canvas', { timeout: 10_000 });
  await page.waitForTimeout(4000);

  const isolated = await page.evaluate(() => window.crossOriginIsolated === true);
  const sabOk = await page.evaluate(() => {
    try {
      new SharedArrayBuffer(16);
      return true;
    } catch {
      return false;
    }
  });

  const pixels = await analyzeCanvas(page);
  await mkdir(ARTIFACTS_DIR, { recursive: true });
  await writeFile(join(ARTIFACTS_DIR, `${name}.png`), await page.locator('#canvas').screenshot());
  await writeFile(join(ARTIFACTS_DIR, `${name}.log.txt`), logs.join('\n'));

  await context.close();
  return { name, logs, isolated, sabOk, pixels };
}

function assert(cond, message) {
  if (!cond) throw new Error(`ASSERT FAILED: ${message}`);
}

async function main() {
  const headed = process.argv.includes('--headed');
  const server = await startServer();

  const executablePath = process.env.CHROME_PATH || undefined;
  const browser = await chromium.launch({
    headless: !headed,
    channel: executablePath ? undefined : 'chrome',
    executablePath,
    args: ['--enable-unsafe-webgpu'],
  });
  if (!executablePath) {
    // channel 'chrome' uses an installed Google Chrome; fall back to
    // Playwright's bundled Chromium if Chrome is not installed.
  }

  const results = [];

  try {
    // ---- Run 1: default — attempt WebGPU, accept either selected path ----
    results.push(await runCase(browser, 'default', []));

    // ---- Run 2: WebGPU disabled — fallback MUST engage -------------------
    const ctx2 = await browser.newContext({ viewport: { width: 800, height: 600 } });
    const page2 = await ctx2.newPage();
    const logs2 = [];
    collectLogs(page2, logs2);
    // Re-open with a flag-forced no-WebGPU environment.
    const browserNoWebGPU = await chromium.launch({
      headless: !headed,
      channel: executablePath ? undefined : 'chrome',
      executablePath,
      args: ['--disable-features=WebGPU', '--disable-webgpu'],
    });
    const ctx = await browserNoWebGPU.newContext({ viewport: { width: 800, height: 600 } });
    const page = await ctx.newPage();
    collectLogs(page, logs2);
    await page.goto(`http://127.0.0.1:${PORT}/index.html`, { waitUntil: 'load' });
    await page.waitForTimeout(4000);
    const pixelsFallback = await analyzeCanvas(page);
    await writeFile(join(ARTIFACTS_DIR, 'no-webgpu.png'), await page.locator('#canvas').screenshot());
    await writeFile(join(ARTIFACTS_DIR, 'no-webgpu.log.txt'), logs2.join('\n'));
    await ctx.close();
    await ctx2.close();
    await browserNoWebGPU.close();
    results.push({
      name: 'no-webgpu',
      logs: logs2,
      pixels: pixelsFallback,
    });
  } finally {
    await browser.close();
    server.close();
  }

  // ---- Assertions --------------------------------------------------------
  const def = results[0];
  assert(def.pixels.total > 0, 'canvas screenshot captured');
  assert(
    def.pixels.golden > def.pixels.total * 0.0005,
    `golden text visible on default run (got ${def.pixels.golden} golden px of ${def.pixels.total})`
  );
  assert(
    def.logs.some((l) => l.includes('AlkALive renderer selected:')),
    'runtime logged its renderer selection'
  );
  assert(def.isolated, 'crossOriginIsolated must be true under COOP/COEP response headers');
  assert(def.sabOk, 'SharedArrayBuffer must be constructible when isolated');

  const nowg = results[1];
  assert(
    nowg.logs.some((l) => l.includes('renderer selected: WebGL2')),
    'fallback run must explicitly select the WebGL2/GLSL renderer'
  );
  assert(
    nowg.pixels.golden > nowg.pixels.total * 0.0005,
    `golden text visible on fallback run (got ${nowg.pixels.golden} golden px of ${nowg.pixels.total})`
  );

  console.log('\n=== AlkALive E2E: ALL ASSERTIONS PASSED ===\n');
  for (const r of results) {
    console.log(
      `[${r.name}] golden=${r.pixels.golden}/${r.pixels.total}px isolated=${r.isolated ?? 'n/a'}`
    );
  }
  const selectionLine = def.logs.find((l) => l.includes('AlkALive renderer selected:'));
  console.log(`Default-run selection: ${selectionLine}`);
}

main().catch((err) => {
  console.error('\n=== AlkALive E2E: FAILURE ===');
  console.error(err.message);
  process.exit(1);
});
