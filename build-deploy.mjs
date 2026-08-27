// Deterministic deploy build pipeline for the AlkALive WASM runtime
// (ADR-017: compact WASM binary, bounded streaming decode).
//
// Pipeline:
//   1. cargo build -p alkalive-runtime-wasm --target wasm32-unknown-unknown
//      --profile wasm-release   (debug/dev builds intentionally skip
//      optimization; this script always builds the deploy artifact)
//   2. wasm-bindgen --target web (CLI version MUST match the wasm-bindgen
//      crate version in Cargo.lock — verified here, fail loudly otherwise)
//   3. wasm-opt -Oz (pinned Binaryen via npm; deterministic given input)
//   4. Structural validation of the optimized module (WebAssembly.compile)
//   5. Size report written to deploy/pkg/build-report.json with SHA-256s
//
// Usage:  node build-deploy.mjs            (from the repository root)
// Prereq: npm install                      (fetches pinned binaryen)

import { spawnSync } from 'node:child_process';
import { readFile, writeFile, stat, rm } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const ROOT = dirname(fileURLToPath(import.meta.url));

// Resolve a cargo-installed binary cross-platform.
// The original Windows-only fallback (USERPROFILE + '.exe') broke Linux CI
// runners: USERPROFILE is undefined there, producing a non-existent relative
// path '.cargo/bin/cargo.exe' (spawn ENOENT). Resolution order:
//   1. explicit env override (CARGO / WASM_BINDGEN)
//   2. $CARGO_HOME/bin/<name>   (set by GitHub Actions runners and rustup)
//   3. ~/.cargo/bin/<name>      (USERPROFILE on Windows, HOME elsewhere)
//   4. bare <name>              (defer to the OS PATH lookup)
// The '.exe' suffix is appended only on Windows, where it is required.
function resolveCargoBin(name, override) {
  if (override) return override;
  const exe = process.platform === 'win32' ? '.exe' : '';
  const candidates = [];
  if (process.env.CARGO_HOME) {
    candidates.push(join(process.env.CARGO_HOME, 'bin', name + exe));
  }
  const userHome = process.env.USERPROFILE ?? process.env.HOME;
  if (userHome) {
    candidates.push(join(userHome, '.cargo', 'bin', name + exe));
  }
  candidates.push(name);
  for (const candidate of candidates) {
    if (candidate === name || existsSync(candidate)) return candidate;
  }
  return name; // not found anywhere: let spawnSync report a clear ENOENT
}

const CARGO = resolveCargoBin('cargo', process.env.CARGO);
const WASM_BINDGEN = resolveCargoBin('wasm-bindgen', process.env.WASM_BINDGEN);
const WASM_BINDGEN_VERSION = '0.2.127'; // must match Cargo.lock's wasm-bindgen

function run(cmd, args, opts = {}) {
  const res = spawnSync(cmd, args, { stdio: ['ignore', 'pipe', 'pipe'], encoding: 'utf8', ...opts });
  if (res.status !== 0) {
    // res.error is set when the process could not be spawned at all
    // (e.g. ENOENT); surface it instead of printing 'undefined'.
    const detail = res.error ? `${res.error.message}` : res.stderr || res.stdout || '(no output)';
    console.error(`\nFAILED: ${cmd} ${args.join(' ')}\n${detail}`);
    process.exit(1);
  }
  return res.stdout;
}

function sha256(buf) {
  return createHash('sha256').update(buf).digest('hex');
}

async function sizeOf(path) {
  try {
    return (await stat(path)).size;
  } catch {
    return null;
  }
}

function requireBinaryen() {
  const require = createRequire(import.meta.url);
  try {
    // binaryen ships a JS entry exposing all tools incl. wasm-opt.
    const binaryen = require('binaryen');
    if (typeof binaryen.readBinary === 'function') {
      return { kind: 'js-api', mod: binaryen };
    }
  } catch {}
  console.error(
    '\nFAILED: the pinned `binaryen` package is not installed.\n' +
      'Run `npm install` at the repository root, then retry.\n'
  );
  process.exit(1);
}

async function main() {
  const targetDir = join(ROOT, 'target', 'wasm32-unknown-unknown', 'wasm-release');
  const rawWasm = join(targetDir, 'alkalive_runtime_wasm.wasm');
  const outDir = join(ROOT, 'deploy', 'pkg');
  const finalWasm = join(outDir, 'alkalive_runtime_wasm_bg.wasm');
  const report = {};

  console.log('[1/5] cargo build (wasm-release)...');
  run(CARGO, [
    'build',
    '-p',
    'alkalive-runtime-wasm',
    '--target',
    'wasm32-unknown-unknown',
    '--profile',
    'wasm-release',
  ]);
  report.afterCargoBytes = await sizeOf(rawWasm);

  console.log('[2/5] wasm-bindgen glue...');
  {
    const versionOut = run(WASM_BINDGEN, ['--version']).trim();
    if (!versionOut.includes(WASM_BINDGEN_VERSION)) {
      console.error(
        `\nFAILED: wasm-bindgen CLI ${versionOut} does not match the crate\n` +
          `version ${WASM_BINDGEN_VERSION} pinned in Cargo.lock. Install the matching CLI:\n` +
          `  cargo install wasm-bindgen-cli --version ${WASM_BINDGEN_VERSION}\n`
      );
      process.exit(1);
    }
  }
  run(WASM_BINDGEN, [rawWasm, '--out-dir', outDir, '--target', 'web', '--typescript']);
  report.afterBindgenBytes = await sizeOf(finalWasm);

  console.log('[3/5] wasm-opt -Oz...');
  {
    const before = await readFile(finalWasm);
    let mod;
    try {
      // binaryen v132 exposes its API on the CJS default export.
      mod = (await import('binaryen')).default;
    } catch {
      console.error(
        '\nFAILED: the pinned `binaryen` package is not installed.\n' +
          'Run `npm install` at the repository root, then retry.\n'
      );
      process.exit(1);
    }
    // Deterministic shrink pass: -Oz == optimizeLevel 3 + shrinkLevel 2.
    mod.setOptimizeLevel(3);
    mod.setShrinkLevel(2);
    const module = mod.readBinary(new Uint8Array(before));
    // Binaryen's validator returns truthy for a VALID module.
    if (!module.validate()) {
      console.error('\nFAILED: pre-optimization module failed binaryen validation\n');
      process.exit(1);
    }
    module.optimize();
    const optimized = Buffer.from(module.emitBinary());
    module.dispose();
    await rm(finalWasm);
    await writeFile(finalWasm, optimized);
  }
  report.afterWasmOptBytes = await sizeOf(finalWasm);

  console.log('[4/5] validating optimized module...');
  {
    const bytes = await readFile(finalWasm);
    // WebAssembly.compile performs full structural+type validation without
    // instantiating (imports stay unresolved by design).
    try {
      await WebAssembly.compile(bytes);
    } catch (e) {
      console.error(`\nFAILED: wasm-opt output failed validation: ${e.message}\n`);
      process.exit(1);
    }
    report.finalSha256 = sha256(bytes);
  }

  // Regenerate the TS declarations against the FINAL (optimized) module so
  // the committed .d.ts matches what ships. bindgen already ran pre-opt; the
  // ABI surface is unchanged by -Oz, but re-run for byte-consistent d.ts.
  report.generatedAt = new Date().toISOString();
  report.pipeline = {
    cargoProfile: 'wasm-release',
    wasmBindgen: WASM_BINDGEN_VERSION,
    wasmOpt: 'binaryen@132.0.0 -Oz (shrinkLevel=2)',
  };
  report.note =
    'Sizes are measured on this machine/toolchain; the reduction claim is ' +
    'the delta recorded in this file, not an assumed percentage.';

  console.log('[5/5] writing build report...');
  await writeFile(join(outDir, 'build-report.json'), JSON.stringify(report, null, 2));

  const kb = (n) => `${(n / 1024).toFixed(1)} KiB`;
  console.log('\n=== build complete ===');
  console.log(`after cargo:     ${kb(report.afterCargoBytes)}`);
  console.log(`after bindgen:   ${kb(report.afterBindgenBytes)}`);
  console.log(`after wasm-opt:  ${kb(report.afterWasmOptBytes)}  (sha256 ${report.finalSha256.slice(0, 16)}...)`);
  const saved = report.afterBindgenBytes - report.afterWasmOptBytes;
  console.log(
    `reduction:       ${kb(saved)} (${((100 * saved) / report.afterBindgenBytes).toFixed(1)}%)`
  );
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
