/* @ts-self-types="./alkalive_app.d.ts" */

/**
 * Check if redo is available.
 * @returns {boolean}
 */
export function can_redo() {
    const ret = wasm.can_redo();
    return ret !== 0;
}

/**
 * Check if undo is available.
 * @returns {boolean}
 */
export function can_undo() {
    const ret = wasm.can_undo();
    return ret !== 0;
}

/**
 * Clear all undo/redo history.
 */
export function clear_history() {
    wasm.clear_history();
}

/**
 * Clear the input field text.
 */
export function clear_input() {
    wasm.clear_input();
}

/**
 * Clear all particles immediately.
 */
export function clear_particles() {
    wasm.clear_particles();
}

/**
 * Clear the current selection (collapse to cursor).
 */
export function clear_selection() {
    wasm.clear_selection();
}

/**
 * Check if a click at (x, y) is within the input field bounds.
 * Returns true if the click hit the input field (and focuses it).
 * Also positions the cursor at the clicked location (hit-test).
 * If `extend` is true (shift+click), extends the selection instead of clearing.
 * @param {number} x
 * @param {number} y
 * @param {boolean} extend
 * @returns {boolean}
 */
export function click_input_field(x, y, extend) {
    const ret = wasm.click_input_field(x, y, extend);
    return ret !== 0;
}

/**
 * Copy the selection to the internal clipboard. Returns the copied text.
 * @returns {string}
 */
export function copy_selection() {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.copy_selection();
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * Cut the selection to the internal clipboard. Returns the cut text.
 * @returns {string}
 */
export function cut_selection() {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.cut_selection();
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * Handle double-click on the input field to select a word.
 * Returns true if the double-click was on the input field.
 * @param {number} x
 * @param {number} y
 * @returns {boolean}
 */
export function double_click_input(x, y) {
    const ret = wasm.double_click_input(x, y);
    return ret !== 0;
}

/**
 * Get the clipboard content.
 * @returns {string}
 */
export function get_clipboard() {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.get_clipboard();
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * Get the current FPS estimate.
 * @returns {number}
 */
export function get_fps() {
    const ret = wasm.get_fps();
    return ret;
}

/**
 * Get the current frame count.
 * @returns {bigint}
 */
export function get_frame_count() {
    const ret = wasm.get_frame_count();
    return BigInt.asUintN(64, ret);
}

/**
 * Get the framebuffer length in bytes.
 * @returns {number}
 */
export function get_framebuffer_len() {
    const ret = wasm.get_framebuffer_len();
    return ret >>> 0;
}

/**
 * Get a raw pointer to the framebuffer data.
 * @returns {number}
 */
export function get_framebuffer_ptr() {
    const ret = wasm.get_framebuffer_ptr();
    return ret >>> 0;
}

/**
 * Get the framebuffer height.
 * @returns {number}
 */
export function get_height() {
    const ret = wasm.get_height();
    return ret >>> 0;
}

/**
 * Get the input field text content.
 * @returns {string}
 */
export function get_input_text() {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.get_input_text();
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * Get the current active particle count.
 * @returns {number}
 */
export function get_particle_count() {
    const ret = wasm.get_particle_count();
    return ret >>> 0;
}

/**
 * Get the current rotation angle in radians (for HUD display).
 * @returns {number}
 */
export function get_rotation_angle() {
    const ret = wasm.get_rotation_angle();
    return ret;
}

/**
 * Get the selected text (empty string if no selection).
 * @returns {string}
 */
export function get_selected_text() {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.get_selected_text();
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * Get the framebuffer width.
 * @returns {number}
 */
export function get_width() {
    const ret = wasm.get_width();
    return ret >>> 0;
}

/**
 * Handle a key press. Returns true if the key was handled.
 *
 * Supported keys:
 * - "Backspace" — delete previous char (or selection)
 * - "Delete" — delete next char (or selection)
 * - "ArrowLeft" — move cursor left
 * - "ArrowRight" — move cursor right
 * - "Home" — move cursor to start
 * - "End" — move cursor to end
 * - "Shift+ArrowLeft" — extend selection left
 * - "Shift+ArrowRight" — extend selection right
 * - "Shift+Home" — extend selection to start
 * - "Shift+End" — extend selection to end
 * - "Ctrl+a" / "Meta+a" — select all
 * - "Ctrl+c" / "Meta+c" — copy selection
 * - "Ctrl+x" / "Meta+x" — cut selection
 * - "Ctrl+v" / "Meta+v" — paste from clipboard
 * - "Enter" — submit (no-op for now, just returns true)
 * - Printable characters — inserted via input_insert_char
 * @param {string} key
 * @returns {boolean}
 */
export function handle_key_press(key) {
    const ptr0 = passStringToWasm0(key, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.handle_key_press(ptr0, len0);
    return ret !== 0;
}

/**
 * Check if the input field has an active selection.
 * @returns {boolean}
 */
export function has_selection() {
    const ret = wasm.has_selection();
    return ret !== 0;
}

/**
 * Initialize the application with the given canvas dimensions.
 * @param {number} width
 * @param {number} height
 */
export function init(width, height) {
    const ret = wasm.init(width, height);
    if (ret[1]) {
        throw takeFromExternrefTable0(ret[0]);
    }
}

/**
 * Insert a character into the input field at the cursor position.
 * Only works if the input field is focused.
 * @param {string} c
 */
export function input_insert_char(c) {
    const ptr0 = passStringToWasm0(c, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    wasm.input_insert_char(ptr0, len0);
}

/**
 * Check if the input field is visible.
 * @returns {boolean}
 */
export function is_input_enabled() {
    const ret = wasm.is_input_enabled();
    return ret !== 0;
}

/**
 * Check if the input field is focused.
 * @returns {boolean}
 */
export function is_input_focused() {
    const ret = wasm.is_input_focused();
    return ret !== 0;
}

/**
 * Check if particles are enabled.
 * @returns {boolean}
 */
export function is_particles_enabled() {
    const ret = wasm.is_particles_enabled();
    return ret !== 0;
}

/**
 * Check if animation is paused.
 * @returns {boolean}
 */
export function is_paused() {
    const ret = wasm.is_paused();
    return ret !== 0;
}

/**
 * Check if rainbow mode is enabled.
 * @returns {boolean}
 */
export function is_rainbow_mode() {
    const ret = wasm.is_rainbow_mode();
    return ret !== 0;
}

/**
 * Handle mouse drag for text selection. Called when the mouse moves while
 * the button is held down (after a click on the input field).
 * Updates the cursor to the dragged position, extending the selection.
 * @param {number} x
 * @param {number} y
 */
export function mouse_drag_input(x, y) {
    wasm.mouse_drag_input(x, y);
}

/**
 * Paste from the internal clipboard at the cursor.
 */
export function paste_clipboard() {
    wasm.paste_clipboard();
}

/**
 * Redo the last undone change. Returns true if redo was performed.
 * @returns {boolean}
 */
export function redo() {
    const ret = wasm.redo();
    return ret !== 0;
}

/**
 * Get the redo stack depth.
 * @returns {number}
 */
export function redo_depth() {
    const ret = wasm.redo_depth();
    return ret >>> 0;
}

/**
 * Resize the framebuffer.
 * @param {number} width
 * @param {number} height
 */
export function resize(width, height) {
    wasm.resize(width, height);
}

/**
 * Select all text in the input field.
 */
export function select_all_input() {
    wasm.select_all_input();
}

/**
 * Set the clipboard content (for external paste from browser clipboard).
 * @param {string} text
 */
export function set_clipboard(text) {
    const ptr0 = passStringToWasm0(text, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    wasm.set_clipboard(ptr0, len0);
}

/**
 * Set solid text color (r, g, b).
 * @param {number} r
 * @param {number} g
 * @param {number} b
 */
export function set_color(r, g, b) {
    wasm.set_color(r, g, b);
}

/**
 * Configure the glow effect: (enabled, radius, intensity).
 * @param {boolean} enabled
 * @param {number} radius
 * @param {number} intensity
 */
export function set_glow(enabled, radius, intensity) {
    wasm.set_glow(enabled, radius, intensity);
}

/**
 * Set vertical gradient: top (r1,g1,b1) to bottom (r2,g2,b2).
 * @param {number} r1
 * @param {number} g1
 * @param {number} b1
 * @param {number} r2
 * @param {number} g2
 * @param {number} b2
 */
export function set_gradient(r1, g1, b1, r2, g2, b2) {
    wasm.set_gradient(r1, g1, b1, r2, g2, b2);
}

/**
 * Toggle the input field visibility.
 * @param {boolean} enabled
 */
export function set_input_enabled(enabled) {
    wasm.set_input_enabled(enabled);
}

/**
 * Set input field focus state.
 * @param {boolean} focused
 */
export function set_input_focus(focused) {
    wasm.set_input_focus(focused);
}

/**
 * Set the input field text directly.
 * @param {string} text
 */
export function set_input_text(text) {
    const ptr0 = passStringToWasm0(text, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    wasm.set_input_text(ptr0, len0);
}

/**
 * Set the particle emission mode.
 * 0 = Off, 1 = TextLine, 2 = RisingSparks, 3 = RadialBurst, 4 = Ambient
 * @param {number} mode
 */
export function set_particle_mode(mode) {
    wasm.set_particle_mode(mode);
}

/**
 * Set the particle emission rate (particles per second, 0-500).
 * @param {number} rate
 */
export function set_particle_rate(rate) {
    wasm.set_particle_rate(rate);
}

/**
 * Toggle particle visibility.
 * @param {boolean} enabled
 */
export function set_particles_enabled(enabled) {
    wasm.set_particles_enabled(enabled);
}

/**
 * Pause or resume the animation.
 * @param {boolean} paused
 */
export function set_paused(paused) {
    wasm.set_paused(paused);
}

/**
 * Enable/disable animated rainbow color mode.
 * When enabled, the text color cycles through the HSV spectrum over time.
 * @param {boolean} enabled
 */
export function set_rainbow_mode(enabled) {
    wasm.set_rainbow_mode(enabled);
}

/**
 * Set the rainbow animation speed (cycles per second, 0-5).
 * @param {number} speed
 */
export function set_rainbow_speed(speed) {
    wasm.set_rainbow_speed(speed);
}

/**
 * Set the rotation speed (radians per second).
 * @param {number} speed
 */
export function set_rotation_speed(speed) {
    wasm.set_rotation_speed(speed);
}

/**
 * Toggle the starfield background.
 * @param {boolean} enabled
 */
export function set_starfield_enabled(enabled) {
    wasm.set_starfield_enabled(enabled);
}

/**
 * Change the rendered text. Re-shapes and re-rasterizes the glyphs.
 * @param {string} text
 */
export function set_text(text) {
    const ptr0 = passStringToWasm0(text, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.set_text(ptr0, len0);
    if (ret[1]) {
        throw takeFromExternrefTable0(ret[0]);
    }
}

/**
 * Render one frame.
 */
export function tick() {
    wasm.tick();
}

/**
 * Toggle input field focus.
 */
export function toggle_input_focus() {
    wasm.toggle_input_focus();
}

/**
 * Undo the last text change. Returns true if undo was performed.
 * @returns {boolean}
 */
export function undo() {
    const ret = wasm.undo();
    return ret !== 0;
}

/**
 * Get the undo stack depth.
 * @returns {number}
 */
export function undo_depth() {
    const ret = wasm.undo_depth();
    return ret >>> 0;
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_throw_344f42d3211c4765: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg_error_744744ff0c9861e6: function(arg0) {
            console.error(arg0);
        },
        __wbindgen_cast_0000000000000001: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./alkalive_app_bg.js": import0,
    };
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = module.ok && expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('alkalive_app_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
