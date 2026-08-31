// Security penetration test (Wave 7, docs/security/07-validation.md): the
// load-time WASM integrity check (T-S1 mitigation, deploy/boot.js).
//
// Attack simulated: the served WASM artifact is tampered with (a single
// flipped byte — the minimum a hostile CDN/proxy/partial-deploy can do).
// Expected defense: boot.js's SHA-256 verification REFUSES to compile the
// module; the console shows the integrity failure and the runtime never
// starts (no window.__alkalive, no render loop).
//
// Run: node tamper-check.mjs   (from test/e2e/)

import { chromium } from 'playwright-core';
import { createServer } from 'node:http';
import { readFile, writeFile, mkdir, rm } from 'node:fs/promises';
import { join, dirname, extname, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomBytes } from 'node:crypto';

const __dirname = dirname(fileURLToPath(import.meta.url));
const DEPLOY_DIR = join(__dirname, '..', '..', 'deploy');
const PORT = 8124;

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript',
  '.mjs': 'text/javascript',
  '.css': 'text/css',
  '.wasm': 'application/wasm',
  '.json': 'application/json',
};

async function makeTamperedDeploy() {
  // Copy deploy/ to a temp dir and flip ONE byte near the end of the
  // module (past any header — content, not structure, so the file is
  // still valid-enough WASM; the digest is what must catch it).
  const tmp = join(__dirname, 'artifacts', 'tampered-deploy');
  await rm(tmp, { recursive: true, force: true });
  await mkdir(join(tmp, 'pkg'), { recursive: true });
  for (const f of [
    'index.html',
    'boot.js',
    'style.css',
    'pkg/alkalive_runtime_wasm.js',
    'pkg/alkalive_runtime_wasm_bg.wasm',
    'pkg/alkalive_runtime_wasm_bg.wasm.d.ts',
    'pkg/alkalive_runtime_wasm.d.ts',
    'pkg/build-report.json',
    'pkg/package.json',
  ]) {
    await writeFile(join(tmp, f), await readFile(join(DEPLOY_DIR, f)));
  }
  const wasmPath = join(tmp, 'pkg', 'alkalive_runtime_wasm_bg.wasm');
  const bytes = await readFile(wasmPath);
  const pos = bytes.length - 64 - (randomBytes(2).readUInt16BE(0) % 32);
  bytes[pos] ^= 0x01;
  await writeFile(wasmPath, bytes);
  return tmp;
}

function startServer(root, port) {
  return new Promise((resolveServer) => {
    const server = createServer(async (req, res) => {
      res.setHeader('Cross-Origin-Opener-Policy', 'same-origin');
      res.setHeader('Cross-Origin-Embedder-Policy', 'require-corp');
      const url = req.url === '/' ? '/index.html' : req.url.split('?')[0];
      if (url === '/favicon.ico') {
        res.statusCode = 204;
        res.end();
        return;
      }
      // Path-traversal guard (same contract as deploy/serve.mjs).
      const target = resolve(join(root, '.' + url));
      if (!target.startsWith(resolve(root) + sep)) {
        res.statusCode = 403;
        res.end('forbidden');
        return;
      }
      try {
        const data = await readFile(target);
        res.setHeader('Content-Type', MIME[extname(url)] ?? 'application/octet-stream');
        res.setHeader('Cache-Control', 'no-store');
        res.end(data);
      } catch {
        res.statusCode = 404;
        res.end('not found');
      }
    });
    server.listen(port, '127.0.0.1', () => resolveServer(server));
  });
}

function assert(cond, message) {
  if (!cond) {
    console.error(`PENETRATION TEST FAILED: ${message}`);
    process.exitCode = 1;
  } else {
    console.log(`  ✓ ${message}`);
  }
}

async function main() {
  const tampered = await makeTamperedDeploy();
  const server = await startServer(tampered, PORT);
  // CI runners ship system Chrome instead of a bundled Playwright browser;
  // ALKALIVE_BROWSER_CHANNEL selects it there (same contract as e2e.mjs).
  const channel = process.env.ALKALIVE_BROWSER_CHANNEL;
  const browser = await chromium.launch({
    headless: true,
    ...(channel ? { channel } : {}),
    args: ['--enable-unsafe-webgpu', '--enable-unsafe-swiftshader', '--use-angle=swiftshader'],
  });
  try {
    const context = await browser.newContext({ viewport: { width: 640, height: 480 } });
    const page = await context.newPage();
    const consoleMessages = [];
    const pageErrors = [];
    page.on('console', (msg) => consoleMessages.push(`[${msg.type()}] ${msg.text()}`));
    page.on('pageerror', (err) => pageErrors.push(String(err)));

    await page.goto(`http://127.0.0.1:${PORT}/index.html`, { waitUntil: 'load' });
    // Give the boot module time to fetch, verify, and refuse.
    await page.waitForTimeout(3000);

    const combined = [...consoleMessages, ...pageErrors].join('\n');
    assert(
      combined.includes('integrity check FAILED'),
      'tampered module triggers the loud integrity failure'
    );
    const runtimeStarted = await page.evaluate(
      () => globalThis.__alkalive !== undefined
    );
    assert(!runtimeStarted, 'runtime REFUSES to start on tampered module (no __alkalive)');

    // Positive control: the UNtampered deploy boots fine (proves the
    // refusal above is caused by the tamper, not by a broken harness).
    const server2 = await startServer(DEPLOY_DIR, PORT + 1);
    const page2 = await (await browser.newContext({ viewport: { width: 640, height: 480 } })).newPage();
    await page2.goto(`http://127.0.0.1:${PORT + 1}/index.html`, { waitUntil: 'load' });
    await page2.waitForTimeout(3000);
    const controlState = await page2.evaluate(() => ({
      alkalive: globalThis.__alkalive ?? null,
    }));
    assert(
      controlState.alkalive !== null,
      `positive control: untampered deploy boots (renderer=${controlState.alkalive?.renderer ?? 'none'})`
    );
    server2.close();
  } finally {
    await browser.close();
    server.close();
  }

  if (process.exitCode) {
    console.error('\n=== TAMPER PENETRATION TEST: FAILED ===');
  } else {
    console.log('\n=== TAMPER PENETRATION TEST: PASSED (T-S1 mitigation verified) ===');
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
