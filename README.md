# trask-technology-site

Marketing site for Trask Technology. The hero draws a clustered graph — hundreds
of thousands of nodes and edges — live in the browser via WebGL2, generated and
rendered from Rust compiled to wasm. Private repo.

```
web/     the page: one self-contained design component, no bundler
  brand/   the one watermark the page loads, copied from ../brand
  pkg/     wasm build output (generated, gitignored)
rust/    graph generation, camera and the WebGL2 renderer, compiled to wasm
brand/   watermark and lockup PNGs (transparent, white and #154F69)
```

`web/` is the deploy unit: everything the page loads at runtime lives under it,
so publishing is a copy of that one directory.

## Running the page

`web/` is static — no build step, no bundler. Serve the folder and open it:

```sh
cd web && python3 -m http.server 8000
```

Then open **`http://localhost:8000/Trask%20Technology%20Site.dc.html`**.

Use `localhost` or `127.0.0.1`, never the machine's LAN or WSL IP. iOS exposes
device orientation only in a secure context, so tilt goes quiet on a plain-http
numeric address. (WebGL2 itself works anywhere.)

`Trask Technology Site.dc.html` is the whole page (markup, styles and logic in
one file), but it no longer renders anything. `GlStage` in Rust owns the WebGL2
context, the camera and its own animation frame and input listeners. What stays
in JavaScript is the markup and controls, the iOS tilt permission prompt (which
has to fire inside a user gesture) and the small 2D canvas preview.

The status line at the top right names the active renderer — `rust/webgl2` or
`canvas2d`. Hover or tap it for the full diagnostic: secure-context state,
backing resolution, and why the Rust path was skipped if it was.

## Rust / wasm

Required — the hero has no JavaScript renderer left. Without the bundle the page
falls back to its static 2D preview.

```sh
cd rust && make          # see `make help` for the other targets
```

One entry point: `GlStage`. See `rust/README.md`.

There was a wgpu/WebGPU renderer alongside it. It was removed — WebGPU has no
point sprites, so a node cost six vertices against WebGL2's one, and the compute
layout pass, the only thing it could do that WebGL2 cannot, was never switched
on. It is in the git history if it is ever wanted back.

## Brand

Navy `#154F69`, Lato. The node-web watermark in `brand/` is generated from the
same algorithm as the hero, exported at 2×–4× with transparency.

`brand/` is the source of truth and holds every variant. Only the one asset the
page actually renders — `trask-watermark-titlebox-white.png`, behind the contact
form — is copied to `web/brand/`, because nothing outside `web/` is served. Swap
the watermark and you need to copy it across again.

## State of things

- The layout is generated once and then held still. Motion is camera only —
  drift, cursor parallax, wheel dolly, device tilt. Animating the layout would
  need transform feedback, since WebGL2 has no compute shaders.
- Node repulsion is approximated against community centroids, not a true
  n-body. Barnes-Hut or a grid hash is the honest next step.
- The Rust renderer was matched against the JavaScript original it replaced.
  The differences that mattered were inherited configuration, not translated
  code: the component's declared `zoom` prop (4, not the `?? 1.7` fallback in the
  source), WebGL's `antialias` defaulting to true, and vertex stride.
- No tracing yet, and the status line reports which renderer won rather than how
  fast it is. Nothing attributes stalls.
