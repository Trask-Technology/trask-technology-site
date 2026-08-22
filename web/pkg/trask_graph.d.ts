/* tslint:disable */
/* eslint-disable */

/**
 * The hero: generation, camera and rendering, all in Rust. `start` registers
 * its own animation frame and input listeners and runs until `stop`.
 */
export class GlStage {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    edge_count(): number;
    node_count(): number;
    set_camera(zoom: number, drift_speed: number, parallax: number, mass: number): void;
    set_params(nodes: number, clusters: number, density: number): void;
    set_pointer(x: number, y: number): void;
    set_tilting(on: boolean): void;
    /**
     * Synchronous: WebGL2 has no adapter to await.
     */
    static start(canvas: HTMLCanvasElement, nodes: number, clusters: number, density: number, seed: number): GlStage;
    stop(): void;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_glstage_free: (a: number, b: number) => void;
    readonly glstage_edge_count: (a: number) => number;
    readonly glstage_node_count: (a: number) => number;
    readonly glstage_set_camera: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly glstage_set_params: (a: number, b: number, c: number, d: number) => void;
    readonly glstage_set_pointer: (a: number, b: number, c: number) => void;
    readonly glstage_set_tilting: (a: number, b: number) => void;
    readonly glstage_start: (a: any, b: number, c: number, d: number, e: number) => [number, number, number];
    readonly glstage_stop: (a: number) => void;
    readonly wasm_bindgen_adaf13b57ad1bb3c___convert__closures_____invoke___f64______true_: (a: number, b: number, c: number) => void;
    readonly wasm_bindgen_adaf13b57ad1bb3c___convert__closures_____invoke___web_sys_5fbc5f797bcb8a0b___features__gen_MouseEvent__MouseEvent______true_: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen_adaf13b57ad1bb3c___convert__closures_____invoke___web_sys_5fbc5f797bcb8a0b___features__gen_MouseEvent__MouseEvent______true__2: (a: number, b: number, c: any) => void;
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
