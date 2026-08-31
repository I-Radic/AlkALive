// AlkALive deployment boot module (security-hardened, docs/security/06-mitigations.md).
//
// Served as an external module script so the page CSP can be strict
// (`script-src 'self' 'wasm-unsafe-eval'`) without any inline-script hashes
// to maintain: every executable byte of the page ships as a same-origin file.
import init from './pkg/alkalive_runtime_wasm.js';

// Security (T-S1): verify the WASM artifact against the SHA-256 recorded by
// the deterministic build pipeline (deploy/pkg/build-report.json, whose
// digest the CI cross-checks against the shipped bytes) BEFORE compiling
// anything. A mismatched, partially-deployed, or tampered module is refused
// loudly instead of being executed.
const [wasmResponse, reportResponse] = await Promise.all([
  fetch('./pkg/alkalive_runtime_wasm_bg.wasm'),
  fetch('./pkg/build-report.json'),
]);
const [wasmBytes, report] = await Promise.all([
  wasmResponse.arrayBuffer(),
  reportResponse.json(),
]);
const digestBytes = new Uint8Array(await crypto.subtle.digest('SHA-256', wasmBytes));
const digestHex = Array.from(digestBytes, (b) => b.toString(16).padStart(2, '0')).join('');
if (digestHex !== report.finalSha256) {
  console.error(
    'AlkALive: WASM integrity check FAILED — refusing to start.\n' +
      `  expected sha256: ${report.finalSha256}\n` +
      `  actual   sha256: ${digestHex}`
  );
  throw new Error('AlkALive: WASM integrity check failed');
}

const wasm = await init({ module_or_path: wasmBytes });
const canvas = document.getElementById('canvas');
const ime = document.getElementById('ime');
// The WASM runtime owns canvas sizing (via devicePixelRatio for high-DPI),
// the resize listener, the frame loop, input forwarding, renderer selection
// (WebGPU primary, WebGL2 fallback) and threading posture.
await wasm.start(canvas, ime);
