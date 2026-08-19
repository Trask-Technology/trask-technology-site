# trask-technology-site

Marketing site for Trask Technology. The hero draws a clustered graph — tens of
thousands of nodes and edges — live in the browser, through WebGPU where the
browser supports it and WebGL2 otherwise. Private repo.

```
web/     the page: one self-contained design component + the WebGPU renderer
rust/    graph generation, GPU force layout and a wgpu renderer, compiled to wasm
brand/   watermark and lockup PNGs (transparent, white and #154F69)
```

## Running the page

`web/` is static — no build step, no bundler. Serve the folder and open it:

```sh
cd web && python3 -m http.server 8000
```

`Trask Technology Site.dc.html` is the whole page (markup, styles and logic in
one file). `gpu-renderer.js` owns the WebGPU path: WGSL shaders, the
compute-shader layout pass, and the render pass that reads node positions
straight out of the storage buffer they live in.

## Rust / wasm

Optional today — the page falls back to an equivalent JS generator when the wasm
bundle is absent, and reports which path ran in the status tooltip under the hero.

```sh
cd rust
wasm-pack build --target web --release --out-dir ../web/pkg
```

Two entry points: `Graph` generates the layout and CSR adjacency for JS to draw;
`Stage` hands the whole pipeline to Rust via `wgpu`. See `rust/README.md`.

`Stage` has not been compiled yet — expect small API drift against your installed
`wgpu` version, `SurfaceTargetUnsafe::from_window` in particular.

## Brand

Navy `#154F69`, Lato. The node-web watermark in `brand/` is generated from the
same algorithm as the hero, exported at 2×–4× with transparency.

## State of things

- Hero layout is generated once and then held still; the compute pass exists and
  is idle. Motion is camera only — drift, cursor parallax, wheel dolly, device tilt.
- Node repulsion is approximated against community centroids, not a true
  n-body. Barnes-Hut or a grid hash is the honest next step.
- No tracing yet: frame time is an EMA of rAF deltas, nothing attributes stalls.
