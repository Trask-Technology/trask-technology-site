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

Then open **`http://localhost:8000/Trask%20Technology%20Site.dc.html`**.

Use `localhost` or `127.0.0.1`, not the machine's LAN or WSL IP. Browsers expose
WebGPU only in a secure context — `https://`, or those two hostnames exactly — so
an address like `http://172.31.29.67:8000` leaves `navigator.gpu` undefined and
the hero falls back to WebGL2 with no error anywhere. If you are serving from
WSL, `localhost` is forwarded from Windows automatically and reaches the same
server.

`Trask Technology Site.dc.html` is the whole page (markup, styles and logic in
one file). `gpu-renderer.js` owns the JS WebGPU path: WGSL shaders, the
compute-shader layout pass, and the render pass that reads node positions
straight out of the storage buffer they live in.

### Which renderer am I getting?

The status line at the top right names it: `rust/wgpu`, `webgpu`, `webgl2` or
`canvas2d`. Hover it for the full breakdown — the generator, the layout pass, the
secure-context state, and the reason each faster path was skipped, if any.

The hero tries them in that order and quietly steps down when one is unavailable,
so the readout is the only way to tell which is actually running.

## Rust / wasm

Optional — the page falls back to an equivalent JS generator when the wasm bundle
is absent, and reports which path ran.

```sh
cd rust
env -u RUSTFLAGS wasm-pack build --target web --release --out-dir ../web/pkg
```

Two entry points: `Graph` generates the layout and CSR adjacency for JS to draw;
`Stage` hands the whole pipeline to Rust via `wgpu`, and requires WebGPU. See
`rust/README.md` — including why `env -u RUSTFLAGS` is there.

## Brand

Navy `#154F69`, Lato. The node-web watermark in `brand/` is generated from the
same algorithm as the hero, exported at 2×–4× with transparency.

## State of things

- Hero layout is generated once and then held still; the compute pass exists and
  is idle. Motion is camera only — drift, cursor parallax, wheel dolly, device tilt.
- Node repulsion is approximated against community centroids, not a true
  n-body. Barnes-Hut or a grid hash is the honest next step.
- Each renderer draws into its own canvas element, because claiming a canvas for
  WebGPU is irreversible and a shared one cannot be handed to a fallback.
- No tracing yet. The status line reports which renderer won, not how fast it is;
  nothing attributes stalls.
