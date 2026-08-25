/* tslint:disable */
/* eslint-disable */

/**
 * The single entry point called from JavaScript.
 *
 * Passes the canvas and hidden IME input element to the WASM runtime. The
 * runtime then owns everything: scene compilation, GPU backend
 * initialization, frame loop, and input handling.
 *
 * # Returns
 *
 * Returns `Ok(())` immediately after kicking off async GPU initialization.
 * The frame loop starts once the renderer is ready. If the `.alk` source
 * fails to compile, returns `Err(JsValue)` synchronously.
 *
 * # JavaScript side
 *
 * ```text
 * import init from './alkalive_runtime.js';
 * const wasm = await init('./alkalive_runtime_bg.wasm');
 * await wasm.start(canvas, ime);
 * ```
 */
export function start(canvas: HTMLCanvasElement, ime_input: HTMLInputElement): void;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly start: (a: number, b: number, c: number) => void;
    readonly __wasm_bindgen_func_elem_12179: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_677: (a: number, b: number, c: number) => void;
    readonly __wasm_bindgen_func_elem_677_2: (a: number, b: number, c: number) => void;
    readonly __wasm_bindgen_func_elem_677_3: (a: number, b: number, c: number) => void;
    readonly __wasm_bindgen_func_elem_676: (a: number, b: number) => void;
    readonly __wbindgen_export: (a: number, b: number) => number;
    readonly __wbindgen_export2: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_export3: (a: number) => void;
    readonly __wbindgen_export4: (a: number, b: number) => void;
    readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
