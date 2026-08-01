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
    readonly start: (a: any, b: any) => [number, number];
    readonly wasm_bindgen_52d8552f206776e2___convert__closures_____invoke___wasm_bindgen_52d8552f206776e2___JsValue__core_9b3796e30d99ddb7___result__Result_____wasm_bindgen_52d8552f206776e2___JsError___true_: (a: number, b: number, c: any) => [number, number];
    readonly wasm_bindgen_52d8552f206776e2___convert__closures_____invoke___web_sys_ce5088807026172e___features__gen_InputEvent__InputEvent______true_: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen_52d8552f206776e2___convert__closures_____invoke___web_sys_ce5088807026172e___features__gen_InputEvent__InputEvent______true__2: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen_52d8552f206776e2___convert__closures_____invoke_______true_: (a: number, b: number) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_destroy_closure: (a: number, b: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
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
