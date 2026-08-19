//! Trask Technology hero graph: generation, GPU force layout, rendering.
//!
//! One entry point for JS: `GlStage`, a WebGL2 renderer via glow.
//!
//! It owns its animation frame and input listeners; the page constructs it and
//! otherwise only forwards slider and tilt changes.
//!
//! There was a wgpu/WebGPU renderer here too. It was removed: WebGPU has no
//! point sprites, so a node cost six vertices against WebGL2's one, and the
//! compute layout pass — the only thing it could do that WebGL2 cannot — was
//! never switched on. See git history if it is wanted back.
//!
//! Build: see the Makefile in this directory.

mod camera;
mod gl;
mod graph;
mod host;

use graph::Graph as G;
use wasm_bindgen::prelude::*;

impl host::Scene for gl::Renderer {
    // the page's JS WebGL2 path used 1.5 * dpr for gl_PointSize; a point sprite
    // and an expanded quad need different values to look the same
    fn point_scale(&self) -> f32 { 1.5 }
    fn resize(&mut self, w: u32, h: u32) { gl::Renderer::resize(self, w, h) }
    fn set_graph(&mut self, g: &G) { gl::Renderer::set_graph(self, g) }
    fn draw(&mut self, rot: [f32; 9], scale: f32, point_px: f32) {
        self.frame(rot, scale, point_px)
    }
}

/// The hero: generation, camera and rendering, all in Rust. `start` registers
/// its own animation frame and input listeners and runs until `stop`.
#[wasm_bindgen]
pub struct GlStage {
    h: host::Handle<gl::Renderer>,
}

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
