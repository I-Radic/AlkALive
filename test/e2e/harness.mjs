// Shared browser-E2E harness: isolation-headers dev server + canvas pixel
// analysis. Used by e2e.mjs (Chromium via Playwright) and
// firefox-e2e.mjs (stock Firefox via WebDriver).

import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { join, extname, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { PNG } from 'pngjs';

const __dirname = dirname(fileURLToPath(import.meta.url));
export const DEPLOY_DIR = join(__dirname, '..', '..', 'deploy');

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript',
  '.mjs': 'text/javascript',
  '.css': 'text/css',
  '.wasm': 'application/wasm',
  '.json': 'application/json',
};

/**
 * Start the deploy server with the ADR-003 COOP/COEP response headers.
 * `<meta http-equiv>` is ignored by browsers for cross-origin isolation;
 * only HTTP response headers count.
 */
export function startServer(port) {
  return new Promise((resolve) => {
    const server = createServer(async (req, res) => {
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
    server.listen(port, () => resolve(server));
  });
}

/** Count black-background and golden-text pixels in a PNG buffer. */
export function analyzePng(buf) {
  const png = PNG.sync.read(buf);
  let black = 0;
  let golden = 0;
  let inputFieldBg = 0;
  const total = png.width * png.height;
  for (let i = 0; i < png.data.length; i += 4) {
    const r = png.data[i];
    const g = png.data[i + 1];
    const b = png.data[i + 2];
    if (r < 8 && g < 8 && b < 8) black++;
    else if (r > 60 && r > g && g > b) golden++;
    else if (Math.abs(r - 13) < 12 && Math.abs(g - 13) < 12 && Math.abs(b - 20) < 14)
      inputFieldBg++;
  }
  return { width: png.width, height: png.height, total, black, golden, inputFieldBg };
}

export function assert(cond, message) {
  if (!cond) throw new Error(`ASSERT FAILED: ${message}`);
}
