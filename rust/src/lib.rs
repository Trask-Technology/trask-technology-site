//! Trask Technology hero graph: generation, GPU force layout, rendering.
//!
//! Two entry points for JS:
//!   * `Graph`   — generation only; JS uploads and draws (used as the fallback path).
//!   * `Stage`   — Rust owns the whole pipeline via wgpu: buffers, compute layout,
//!                 render passes. JS hands over a canvas and calls `frame()`.
//!
//! Build: wasm-pack build --target web --release --out-dir ../pkg

mod graph;
#[cfg(any(feature = "render", feature = "webgl"))]
mod camera;
#[cfg(feature = "webgl")]
mod gl;
#[cfg(any(feature = "render", feature = "webgl"))]
mod host;
#[cfg(feature = "render")]
mod gpu;

use graph::Graph as G;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Graph {
    inner: G,
}

#[wasm_bindgen]
impl Graph {
    /// `nodes` vertices in `clusters` communities; `density` scales edge count.
    #[wasm_bindgen(constructor)]
    pub fn new(nodes: usize, clusters: usize, density: f32, seed: u32) -> Graph {
        Graph { inner: G::generate(nodes, clusters, density, seed) }
    }

    pub fn node_count(&self) -> usize { self.inner.nodes }
    pub fn edge_count(&self) -> usize { self.inner.edge_count() }
    pub fn cluster_count(&self) -> usize { self.inner.clusters }

    /// `nodes * 4` f32: xyz + community index. Views into wasm memory — no copy.
    pub fn positions_ptr(&self) -> *const f32 { self.inner.pos.as_ptr() }
    pub fn edges_ptr(&self) -> *const u32 { self.inner.edges.as_ptr() }
    pub fn csr_off_ptr(&self) -> *const u32 { self.inner.csr_off.as_ptr() }
    pub fn csr_nbr_ptr(&self) -> *const u32 { self.inner.csr_nbr.as_ptr() }
    pub fn csr_nbr_len(&self) -> usize { self.inner.csr_nbr.len() }
    pub fn centroids_ptr(&self) -> *const f32 { self.inner.centroids.as_ptr() }
}

#[cfg(feature = "render")]
impl host::Scene for gpu::Renderer {
    fn point_scale(&self) -> f32 { 1.1 }
    fn resize(&mut self, w: u32, h: u32) { gpu::Renderer::resize(self, w, h) }
    fn set_graph(&mut self, g: &G) { gpu::Renderer::set_graph(self, g) }
    fn draw(&mut self, rot: [f32; 9], scale: f32, point_px: f32, simulate: bool) {
        self.frame(rot, scale, point_px, simulate, 0.016)
    }
}

#[cfg(feature = "webgl")]
impl host::Scene for gl::Renderer {
    // matches the page's JS WebGL2 path, which uses 1.5 * dpr for gl_PointSize
    fn point_scale(&self) -> f32 { 1.5 }
    fn resize(&mut self, w: u32, h: u32) { gl::Renderer::resize(self, w, h) }
    fn set_graph(&mut self, g: &G) { gl::Renderer::set_graph(self, g) }
    // WebGL2 has no compute, so there is no layout pass to run here.
    fn draw(&mut self, rot: [f32; 9], scale: f32, point_px: f32, _simulate: bool) {
        self.frame(rot, scale, point_px)
    }
}

/// Rust-owned hero on WebGPU. `start` builds the pipeline, registers its own
/// animation frame and input listeners, and runs until `stop()`.
#[cfg(feature = "render")]
#[wasm_bindgen]
pub struct Stage {
    h: host::Handle<gpu::Renderer>,
}

#[cfg(feature = "render")]
#[wasm_bindgen]
impl Stage {
    /// Async because adapter and device acquisition are.
    pub async fn start(
        canvas: web_sys::HtmlCanvasElement,
        nodes: usize,
        clusters: usize,
        density: f32,
        seed: u32,
    ) -> Result<Stage, JsValue> {
        let g = G::generate(nodes, clusters, density, seed);
        let (w, h) = (canvas.width().max(1), canvas.height().max(1));
        let renderer = gpu::Renderer::new(wgpu::SurfaceTarget::Canvas(canvas.clone()), w, h, &g)
            .await
            .map_err(|e| JsValue::from_str(&e))?;
        Ok(Stage { h: host::Handle::start(host::Host::new(renderer, canvas, &g, clusters, density, seed)) })
    }

    pub fn stop(&mut self) { self.h.stop() }
    pub fn set_params(&self, nodes: usize, clusters: usize, density: f32) { self.h.set_params(nodes, clusters, density) }
    /// Device tilt: a direct 1:1 look-around. The page owns the iOS permission
    /// prompt, which has to happen inside a user gesture.
    pub fn set_camera(&self, zoom: f32, drift_speed: f32, parallax: f32, mass: f32) {
        self.h.set_camera(zoom, drift_speed, parallax, mass)
    }
    pub fn set_tilting(&self, on: bool) { self.h.set_tilting(on) }
    /// Tilt input, both axes in -1..1. Ignored unless `set_tilting(true)`.
    pub fn set_pointer(&self, x: f32, y: f32) { self.h.set_pointer(x, y) }
    /// Run the WGSL force layout each frame. Off by default.
    pub fn set_simulate(&self, on: bool) { self.h.set_simulate(on) }
    pub fn node_count(&self) -> usize { self.h.nodes() }
    pub fn edge_count(&self) -> usize { self.h.edges() }
}

/// The same hero on WebGL2. Same API as `Stage`, no wgpu, and nodes are real
/// point sprites rather than expanded quads.
#[cfg(feature = "webgl")]
#[wasm_bindgen]
pub struct GlStage {
    h: host::Handle<gl::Renderer>,
}

#[cfg(feature = "webgl")]
#[wasm_bindgen]
impl GlStage {
    /// Synchronous: WebGL2 has no adapter to await.
    pub fn start(
        canvas: web_sys::HtmlCanvasElement,
        nodes: usize,
        clusters: usize,
        density: f32,
        seed: u32,
    ) -> Result<GlStage, JsValue> {
        let g = G::generate(nodes, clusters, density, seed);
        let renderer = gl::Renderer::new(&canvas, &g).map_err(|e| JsValue::from_str(&e))?;
        Ok(GlStage { h: host::Handle::start(host::Host::new(renderer, canvas, &g, clusters, density, seed)) })
    }

    pub fn stop(&mut self) { self.h.stop() }
    pub fn set_params(&self, nodes: usize, clusters: usize, density: f32) { self.h.set_params(nodes, clusters, density) }
    pub fn set_camera(&self, zoom: f32, drift_speed: f32, parallax: f32, mass: f32) {
        self.h.set_camera(zoom, drift_speed, parallax, mass)
    }
    pub fn set_tilting(&self, on: bool) { self.h.set_tilting(on) }
    pub fn set_pointer(&self, x: f32, y: f32) { self.h.set_pointer(x, y) }
    pub fn node_count(&self) -> usize { self.h.nodes() }
    pub fn edge_count(&self) -> usize { self.h.edges() }
}
