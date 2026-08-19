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
wasm-pack build --target web --release --out-dir ../web/pkg
```

Writes `trask_graph.js` + `trask_graph_bg.wasm` into `web/web/pkg/`, beside
`Trask Technology Site.dc.html`, which imports them from `./pkg/`. The page then
reports `layout: rust/wasm` and `draw: rust/wgpu` in the status tooltip under the
hero; without the build it falls back to the JS generator.

Generation-only bundle — no wgpu, no `Stage`, ~27 KB instead of ~200 KB:

```sh
wasm-pack build --target web --release --no-default-features --out-dir ../web/pkg
```

Note that this bundle has no `Stage` export, so the page will report `webgpu` or
`webgl2` rather than `rust/wgpu`. That is the expected result, not a failure.

### A global `RUSTFLAGS` will break this

`RUSTFLAGS` applies to every target, `wasm32-unknown-unknown` included. A host
tuning flag such as `-C target-cpu=native` leaks into the wasm build, and
wasm-bindgen then fails with `failed to find intrinsics to enable clone_ref`.
Clear it for the build:

```sh
env -u RUSTFLAGS wasm-pack build --target web --release --out-dir ../web/pkg
```

Better, keep host tuning out of the environment entirely and scope it to the host
target in `~/.cargo/config.toml`, where it cannot reach a wasm build:

```toml
[target.x86_64-unknown-linux-gnu]
rustflags = ["-C", "target-cpu=native"]
```

## WebGPU only

The `render` feature builds wgpu with the `webgpu` backend alone. The layout pass
is a compute shader and WebGL2 has none, so a GL adapter could only fail later, at
pipeline creation. With WebGPU absent the adapter request returns nothing,
`Stage::create` rejects cleanly, and the page falls back to its JS renderers.

The browser exposes `navigator.gpu` only in a **secure context**: `https://`, or
`http://localhost` / `http://127.0.0.1` exactly. Serving the page on a LAN or WSL
IP over plain http hides WebGPU from the page altogether, and the hero drops to
WebGL2 with nothing obviously wrong. See the root README for how to serve it.

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

`Stage::create` takes ownership of the canvas's WebGPU context, and a canvas can
never hand its context back. Give `Stage` its own canvas element rather than one
another renderer may need to fall back onto.

## Native build

Not currently possible. `Cargo.toml` enables no native wgpu backend, and
`Stage::create` takes a `web_sys::HtmlCanvasElement`. Running the layout outside
the browser — worth doing to profile at sizes a tab will not tolerate — needs a
`vulkan`/`metal`/`dx12` feature and a `cfg`-split surface target first. There is
no `examples/` directory yet.

## Not yet done

* `simd` is declared in `Cargo.toml` but nothing is behind `cfg(feature = "simd")`,
  so enabling it — or building with `-C target-feature=+simd128` — changes nothing.
* Barnes-Hut or a grid hash would replace centroid repulsion for a layout that is
  correct rather than merely plausible.
* Timestamp queries for real GPU-side frame timing.
* No tests, and `Stage` has had very little real-world exposure. The shaders
  validate under naga and the pipeline builds, but treat wgpu validation errors as
  expected rather than surprising.
