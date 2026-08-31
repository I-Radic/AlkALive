// AlkALive deployment static server.
//
// Serves `deploy/` with the cross-origin-isolation response headers required
// by ADR-003/ADR-021:
//
//   Cross-Origin-Opener-Policy:    same-origin
//   Cross-Origin-Embedder-Policy:  require-corp
//
// These MUST be HTTP response headers. <meta http-equiv> equivalents are
// ignored by browsers for isolation purposes, so serving this directory with
// a plain static file server leaves SharedArrayBuffer unavailable.
//
// Usage: node deploy/serve.mjs [port]   (default 8080)

import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { join, resolve, extname, dirname, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname);
const PORT = Number(process.argv[2]) || 8080;

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript',
  '.mjs': 'text/javascript',
  '.css': 'text/css',
  '.wasm': 'application/wasm',
  '.json': 'application/json',
  '.ts': 'text/plain',
  '.ttf': 'font/ttf',
};

createServer(async (req, res) => {
  // ADR-003/ADR-021: cross-origin isolation via HTTP RESPONSE headers.
  res.setHeader('Cross-Origin-Opener-Policy', 'same-origin');
  res.setHeader('Cross-Origin-Embedder-Policy', 'require-corp');
  // Security hardening (docs/security/06-mitigations.md, T-I2 family):
  // - nosniff: prevents MIME confusion of served files
  // - DENY framing: the app is a standalone canvas, never a frame target
  //   (belt-and-suspenders next to the page's own CSP, which cannot carry
  //   frame-ancestors in a <meta>)
  // - no-referrer: nothing off-origin is ever fetched; nothing to leak
  // - permissions lockdown: no powerful APIs are used or granted
  res.setHeader('X-Content-Type-Options', 'nosniff');
  res.setHeader('X-Frame-Options', 'DENY');
  res.setHeader('Referrer-Policy', 'no-referrer');
  res.setHeader('Permissions-Policy', 'camera=(), microphone=(), geolocation=(), payment=(), usb=(), serial=(), bluetooth=()');

  const url = req.url === '/' ? '/index.html' : req.url.split('?')[0];
  if (url === '/favicon.ico') {
    res.statusCode = 204;
    res.end();
    return;
  }
  // Path-traversal guard: resolve and verify the target stays in deploy/.
  const target = resolve(join(__dirname, '.' + url));
  if (!target.startsWith(ROOT + sep)) {
    res.statusCode = 403;
    res.end('forbidden');
    return;
  }
  try {
    const data = await readFile(target);
    res.setHeader('Content-Type', MIME[extname(url)] ?? 'application/octet-stream');
    res.setHeader('Content-Length', data.length);
    res.end(data);
  } catch {
    res.statusCode = 404;
    res.end('not found');
  }
}).listen(PORT, '127.0.0.1', () => {
  console.log(`AlkALive deploy server: http://127.0.0.1:${PORT}/ (COOP/COEP set)`);
});
