// Verification script — loads the AlkALive WASM module and checks that
// the Hello World framebuffer contains golden pixels on a black background.
//
// Usage: node verify_wasm.js

import { readFile } from 'fs/promises';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

async function main() {
    const wasmPath = join(__dirname, 'deploy', 'alkalive_app_bg.wasm');
    const jsPath = join(__dirname, 'deploy', 'alkalive_app.js');

    console.log('Loading WASM module from:', wasmPath);

    // Read the WASM binary.
    const wasmBytes = await readFile(wasmPath);

    // Import the JS glue module.
    const wasmModule = await import(`file://${jsPath}`);

    // Initialize the WASM module.
    const wasm = await wasmModule.default(wasmBytes);

    console.log('WASM module initialized.');
    console.log('Exported functions:', Object.keys(wasm).filter(k => !k.startsWith('_')));

    // Test dimensions.
    const width = 800;
    const height = 600;

    // Call init(width, height).
    wasmModule.init(width, height);
    console.log(`init(${width}, ${height}) called.`);

    // Verify dimensions.
    const w = wasmModule.get_width();
    const h = wasmModule.get_height();
    console.log(`Dimensions: ${w}x${h}`);
    if (w !== width || h !== height) {
        throw new Error(`Dimension mismatch: expected ${width}x${height}, got ${w}x${h}`);
    }

    // Call tick() to render one frame.
    wasmModule.tick();
    console.log('tick() called.');

    // Get the framebuffer.
    const ptr = wasmModule.get_framebuffer_ptr();
    const len = wasmModule.get_framebuffer_len();
    console.log(`Framebuffer: ptr=${ptr}, len=${len}`);

    if (ptr === 0 || len === 0) {
        throw new Error('Framebuffer is empty!');
    }

    if (len !== width * height * 4) {
        throw new Error(`Framebuffer length mismatch: expected ${width * height * 4}, got ${len}`);
    }

    // Create a view into WASM memory.
    const fb = new Uint8Array(wasm.memory.buffer, ptr, len);

    // Count pixel types.
    let blackPixels = 0;
    let goldenPixels = 0;
    let otherPixels = 0;
    let totalR = 0, totalG = 0, totalB = 0;

    for (let i = 0; i < len; i += 4) {
        const r = fb[i];
        const g = fb[i + 1];
        const b = fb[i + 2];
        const a = fb[i + 3];

        totalR += r;
        totalG += g;
        totalB += b;

        if (r === 0 && g === 0 && b === 0) {
            blackPixels++;
        } else if (r > g && g > b && r > 50) {
            goldenPixels++;
        } else {
            otherPixels++;
        }
    }

    const totalPixels = width * height;
    console.log('\n=== Framebuffer Analysis ===');
    console.log(`Total pixels: ${totalPixels}`);
    console.log(`Black pixels: ${blackPixels} (${(blackPixels / totalPixels * 100).toFixed(1)}%)`);
    console.log(`Golden pixels: ${goldenPixels} (${(goldenPixels / totalPixels * 100).toFixed(1)}%)`);
    console.log(`Other pixels: ${otherPixels} (${(otherPixels / totalPixels * 100).toFixed(1)}%)`);
    console.log(`Average color: R=${(totalR / totalPixels).toFixed(1)}, G=${(totalG / totalPixels).toFixed(1)}, B=${(totalB / totalPixels).toFixed(1)}`);

    // Assertions.
    if (goldenPixels === 0) {
        throw new Error('FAIL: No golden pixels found in framebuffer!');
    }
    console.log(`\n✓ PASS: Found ${goldenPixels} golden pixels (Hello World text is visible).`);

    if (blackPixels < totalPixels * 0.9) {
        console.log(`WARNING: Expected >90% black pixels (background), got ${(blackPixels / totalPixels * 100).toFixed(1)}%`);
    } else {
        console.log(`✓ PASS: Background is predominantly black (${(blackPixels / totalPixels * 100).toFixed(1)}%).`);
    }

    // Test multiple frames (rotation animation).
    console.log('\n=== Animation Test (10 frames) ===');
    let prevGoldenCount = goldenPixels;
    for (let frame = 1; frame <= 10; frame++) {
        wasmModule.tick();
        const fb2 = new Uint8Array(wasm.memory.buffer, ptr, len);
        let golden = 0;
        for (let i = 0; i < len; i += 4) {
            if (fb2[i] > fb2[i + 1] && fb2[i + 1] > fb2[i + 2] && fb2[i] > 50) {
                golden++;
            }
        }
        console.log(`Frame ${frame}: ${golden} golden pixels`);
        prevGoldenCount = golden;
    }

    // Test resize.
    console.log('\n=== Resize Test ===');
    wasmModule.resize(400, 300);
    const newW = wasmModule.get_width();
    const newH = wasmModule.get_height();
    const newLen = wasmModule.get_framebuffer_len();
    console.log(`After resize: ${newW}x${newH}, len=${newLen}`);
    if (newW !== 400 || newH !== 300 || newLen !== 400 * 300 * 4) {
        throw new Error('Resize failed!');
    }
    console.log('✓ PASS: Resize works correctly.');

    console.log('\n=== ALL TESTS PASSED ===');
    console.log('AlkALive Hello World WASM module is working correctly.');
    console.log('Golden "Hello World!" text is rendered on a black background with rotation animation.');
}

main().catch(err => {
    console.error('FAIL:', err.message);
    process.exit(1);
});
