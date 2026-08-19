# trask_graph — hero graph: generation, camera, WebGL2 rendering

One entry point, `GlStage`. It generates the graph, owns the WebGL2 context and
the camera, registers its own animation frame and pointer/wheel listeners, and
runs until `stop`. The page constructs it and otherwise only forwards slider and
tilt changes.

Nodes are drawn as **point sprites** — `gl_PointSize` plus a circular `discard`
on `gl_PointCoord` — so a node costs one vertex. That is the reason this is
WebGL2 and not WebGPU: WGSL has no point size, so the same graph would need a
six-vertex quad per node, 1.2M vertices against 200k at 200k nodes.

## Build

```sh
make          # into ../web/pkg
make help     # the other targets
```

### A global `RUSTFLAGS` will break this

`RUSTFLAGS` applies to every target, `wasm32-unknown-unknown` included. A host
tuning flag such as `-C target-cpu=native` leaks into the wasm build and
wasm-bindgen then fails with `failed to find intrinsics to enable clone_ref`.
The Makefile strips it. Better, scope host tuning to the host target in
`~/.cargo/config.toml`, where it cannot reach a wasm build:

```toml
[target.x86_64-unknown-linux-gnu]
rustflags = ["-C", "target-cpu=native"]
```

## JS usage

```js
import init, { GlStage } from './pkg/trask_graph.js';
await init();

const stage = GlStage.start(canvas, 250000, 96, 1.0, 1337);
stage.set_camera(zoom, driftSpeed, parallax, mass);
stage.set_params(nodes, clusters, density);   // regenerates on the live context
stage.set_tilting(true);                      // then feed set_pointer from tilt
stage.stop();                                 // detaches frame and listeners
```

There is no per-frame call to make. One thing is easy to get wrong: pass the
**host's** camera props to `set_camera`. A framework's declared defaults are not
necessarily the `??` fallbacks written in the calling code, and getting that
wrong silently rescales the whole scene.

The context is created with `antialias: false, alpha: false` to match what the
page needs — MSAA both softens 1px lines and point sprites and costs real fill
rate at 5K, and an alpha channel would composite through the wrapper's CSS mask
on top of the fade that mask already applies.

## Layout

`Graph::generate` places `clusters` community centroids on a sphere shell, draws
members around each as a gaussian cloud, wires a ring plus random intra-cluster
edges and five bridges per cluster, then builds CSR adjacency by counting sort.
Deterministic in `seed`.

The layout is generated once and held still; motion is camera only. Animating it
would need transform feedback, since WebGL2 has no compute shaders. There was a
WGSL compute force layout here when the crate also had a wgpu renderer — see git
history.

## Native build

`glow` compiles against native OpenGL, so a native build is mostly a matter of
supplying a context and a window instead of a canvas. Nothing does that yet;
`GlStage::start` takes a `web_sys::HtmlCanvasElement`.

## Not yet done

* Barnes-Hut or a grid hash would replace centroid repulsion if the layout is
  ever animated, for something correct rather than merely plausible.
* No frame timing beyond the browser's own tools; nothing attributes stalls.
* No tests.
