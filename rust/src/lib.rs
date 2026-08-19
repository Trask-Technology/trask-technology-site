//! Trask Technology hero graph: generation, GPU force layout, rendering.
//!
//! Two entry points for JS:
//!   * `Graph`   — generation only; JS uploads and draws (used as the fallback path).
//!   * `Stage`   — Rust owns the whole pipeline via wgpu: buffers, compute layout,
//!                 render passes. JS hands over a canvas and calls `frame()`.
//!
//! Build: wasm-pack build --target web --release --out-dir ../pkg

mod graph;
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

/// Rust-owned renderer. `Stage::create` is async because adapter/device
/// acquisition is; await it before the first `frame()`.
#[cfg(feature = "render")]
#[wasm_bindgen]
pub struct Stage {
    renderer: gpu::Renderer,
    nodes: usize,
    edges: usize,
}

#[cfg(feature = "render")]
#[wasm_bindgen]
impl Stage {
    #[wasm_bindgen]
    pub async fn create(
        canvas: web_sys::HtmlCanvasElement,
        nodes: usize,
        clusters: usize,
        density: f32,
        seed: u32,
    ) -> Result<Stage, JsValue> {
        let g = G::generate(nodes, clusters, density, seed);
        let (w, h) = (canvas.width().max(1), canvas.height().max(1));
        let target = wgpu::SurfaceTarget::Canvas(canvas);
        let renderer = gpu::Renderer::new(target, w, h, &g)
            .await
            .map_err(|e| JsValue::from_str(&e))?;
        Ok(Stage { renderer, nodes: g.nodes, edges: g.edge_count() })
    }

    pub fn node_count(&self) -> usize { self.nodes }
    pub fn edge_count(&self) -> usize { self.edges }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.renderer.resize(width, height);
    }

    /// `rot` is a column-major 3x3 rotation, 9 floats.
    pub fn frame(&mut self, rot: &[f32], scale: f32, point_px: f32, simulate: bool, dt: f32) {
        if rot.len() < 9 { return; }
        let mut r = [0.0f32; 9];
        r.copy_from_slice(&rot[..9]);
        self.renderer.frame(r, scale, point_px, simulate, dt);
    }
}
