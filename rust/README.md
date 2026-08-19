# trask_graph — hero graph: generation, GPU layout, rendering

Two ways in, both exported to wasm:

| Entry | Owns | Used when |
|---|---|---|
| `Graph` | generation + CSR adjacency | JS draws (WebGPU or WebGL2 path) |
| `Stage` | the whole pipeline via `wgpu` — buffers, compute layout, render passes | Rust drives the GPU |

The force layout is a WGSL compute shader (`src/shaders/sim.wgsl`): one invocation
per node, springs along the node's CSR neighbour slice, repulsion against community
centroids (O(clusters), not O(n²)), then integration. Positions live in a storage
buffer that the render pass binds directly as a vertex buffer — nothing is copied
back to the CPU between frames.

## Build

```sh
cargo install wasm-pack                     # once
cd rust
wasm-pack build --target web --release --out-dir ../pkg
```

Writes `pkg/trask_graph.js` + `pkg/trask_graph_bg.wasm` at the project root, where
`Trask Technology Site.dc.html` looks for them. The page then reports
`layout: rust/wasm` (and `draw: rust/wgpu` once `Stage` is in use) in the status
tooltip under the hero; without the build it falls back to the JS generator.

Generation-only bundle (much smaller, no wgpu):

```sh
wasm-pack build --target web --release --no-default-features --out-dir ../pkg
```

SIMD (128-bit) for the generation pass:

```sh
RUSTFLAGS="-C target-feature=+simd128" wasm-pack build --target web --release --out-dir ../pkg
```

## Native build

The same crate runs outside the browser — `wgpu` selects Vulkan, Metal or DX12,
and the WGSL is unchanged. Useful for profiling the layout at sizes a tab won't
tolerate:

```sh
cargo run --release --example bench     # add your own example/bin
```

## JS usage

```js
import init, { Graph, Stage } from './pkg/trask_graph.js';
const wasm = await init();

// generation only
const g = new Graph(250000, 96, 1.0, 1337);
const pos = new Float32Array(wasm.memory.buffer, g.positions_ptr(), g.node_count() * 4);

// or hand Rust the canvas and let it drive
const stage = await Stage.create(canvas, 250000, 96, 1.0, 1337);
stage.frame(rot3x3, scale, pointPx, true, 0.016);
```

Re-create any `Float32Array`/`Uint32Array` views after a call that can grow wasm
memory — the backing buffer may have been reallocated.

## Not yet done

* `Stage` is written but has never been compiled here — expect small API drift
  against your installed `wgpu` version (`SurfaceTargetUnsafe::from_window` in
  particular moves between releases).
* Barnes-Hut or a grid hash would replace centroid repulsion for a layout that is
  correct rather than merely plausible.
* Timestamp queries for real GPU-side frame timing instead of rAF deltas.
