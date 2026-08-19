//! Shared browser host: owns the animation frame, the input listeners and the
//! canvas sizing, for whichever renderer is driving.
//!
//! Both the wgpu and the WebGL2 renderers want exactly the same plumbing, and
//! the camera is already renderer-independent, so only the drawing differs.
//! That difference is the `Scene` trait; everything else lives here once.

use crate::camera::Camera;
use crate::graph::Graph;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

pub trait Scene {
    /// Dot size in CSS pixels, before dpr. The two renderers draw a node
    /// differently — a point sprite against an expanded quad — and need
    /// different values to look the same, so each states its own.
    fn point_scale(&self) -> f32;
    fn resize(&mut self, width: u32, height: u32);
    fn set_graph(&mut self, graph: &Graph);
    fn draw(&mut self, rot: [f32; 9], scale: f32, point_px: f32, simulate: bool);
}

/// WebGPU's default maxTextureDimension2D. A 5K window at dpr 1.5 (7680) comes
/// close enough that an unclamped size would fail surface configuration.
const MAX_DIM: i32 = 8192;

pub fn window() -> web_sys::Window {
    web_sys::window().expect("no window")
}

pub fn now_ms() -> f64 {
    window().performance().map(|p| p.now()).unwrap_or(0.0)
}

pub struct Host<S: Scene> {
    pub scene: S,
    pub camera: Camera,
    pub canvas: web_sys::HtmlCanvasElement,
    pub nodes: usize,
    pub edges: usize,
    pub clusters: usize,
    pub density: f32,
    pub seed: u32,
    pub simulate: bool,
    pub running: bool,
}

impl<S: Scene> Host<S> {
    pub fn new(scene: S, canvas: web_sys::HtmlCanvasElement, g: &Graph, clusters: usize, density: f32, seed: u32) -> Host<S> {
        Host {
            scene,
            camera: Camera::new(now_ms()),
            canvas,
            nodes: g.nodes,
            edges: g.edge_count(),
            clusters,
            density,
            seed,
            simulate: false,
            running: true,
        }
    }

    fn dpr(&self) -> f32 {
        // Matches the page exactly: Math.min(devicePixelRatio || 1, 1.5).
        // There is no lower clamp — a browser zoomed out reports dpr < 1, and
        // forcing it up to 1 would render at a higher resolution than the JS
        // path, changing the apparent size of 1px lines and point sprites.
        let d = window().device_pixel_ratio() as f32;
        let d = if d > 0.0 { d } else { 1.0 };
        d.min(1.5)
    }

    fn frame(&mut self, now: f64) {
        if window().document().map(|d| d.hidden()).unwrap_or(false) {
            return;
        }
        // scrolled past the hero — nothing worth drawing
        if self.canvas.get_bounding_client_rect().bottom() < 0.0 {
            return;
        }

        // wgpu's webgpu backend does not touch canvas.width/height; the drawing
        // buffer is whatever those attributes say, so they have to track the size
        // the renderer is configured for. WebGL2 needs the same for its viewport.
        let dpr = self.dpr();
        let w = ((self.canvas.client_width() as f32 * dpr).round() as i32).min(MAX_DIM);
        let h = ((self.canvas.client_height() as f32 * dpr).round() as i32).min(MAX_DIM);
        if w > 0 && h > 0 {
            if self.canvas.width() != w as u32 || self.canvas.height() != h as u32 {
                self.canvas.set_width(w as u32);
                self.canvas.set_height(h as u32);
            }
            self.scene.resize(w as u32, h as u32);
        }

        let v = self.camera.step(now);
        self.scene.draw(v.rot, v.scale, self.scene.point_scale() * dpr, self.simulate);
    }
}

/// Owns the closures. Dropping them is what stops the loop, so they are held
/// here rather than leaked.
pub struct Handle<S: Scene + 'static> {
    pub inner: Rc<RefCell<Host<S>>>,
    raf: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>>,
    on_move: Option<Closure<dyn FnMut(web_sys::MouseEvent)>>,
    on_wheel: Option<Closure<dyn FnMut(web_sys::WheelEvent)>>,
}

fn request_frame(cb: &Closure<dyn FnMut(f64)>) {
    let _ = window().request_animation_frame(cb.as_ref().unchecked_ref());
}

impl<S: Scene + 'static> Handle<S> {
    pub fn start(host: Host<S>) -> Handle<S> {
        let inner = Rc::new(RefCell::new(host));

        // The closure holds the Rc that holds the closure; that cycle is what
        // keeps it alive across frames, and `stop` is what breaks it.
        let raf: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
        {
            let raf_inner = raf.clone();
            let state = inner.clone();
            *raf.borrow_mut() = Some(Closure::wrap(Box::new(move |now: f64| {
                {
                    let mut s = state.borrow_mut();
                    if !s.running {
                        return;
                    }
                    s.frame(now);
                }
                if let Some(cb) = raf_inner.borrow().as_ref() {
                    request_frame(cb);
                }
            }) as Box<dyn FnMut(f64)>));
            if let Some(cb) = raf.borrow().as_ref() {
                request_frame(cb);
            }
        }

        let state = inner.clone();
        let on_move = Closure::wrap(Box::new(move |e: web_sys::MouseEvent| {
            let win = window();
            let iw = win.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1.0);
            let ih = win.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(1.0);
            if let Ok(mut s) = state.try_borrow_mut() {
                // device tilt wins while it is driving
                if s.camera.tilting {
                    return;
                }
                s.camera.mx = ((e.client_x() as f64 / iw - 0.5) * 2.0) as f32;
                s.camera.my = ((e.client_y() as f64 / ih - 0.5) * 2.0) as f32;
            }
        }) as Box<dyn FnMut(web_sys::MouseEvent)>);
        let opts = web_sys::AddEventListenerOptions::new();
        opts.set_passive(true);
        let _ = window().add_event_listener_with_callback_and_add_event_listener_options(
            "pointermove",
            on_move.as_ref().unchecked_ref(),
            &opts,
        );

        // Not passive: at either depth limit the page takes the scroll back, so
        // preventDefault has to remain available.
        let state = inner.clone();
        let on_wheel = Closure::wrap(Box::new(move |e: web_sys::WheelEvent| {
            if window().scroll_y().unwrap_or(0.0) > 40.0 {
                return;
            }
            if let Ok(mut s) = state.try_borrow_mut() {
                let target = s.camera.dolly_target;
                let fwd = e.delta_y() > 0.0;
                if (fwd && target >= 1.45) || (!fwd && target <= 1.0) {
                    return;
                }
                e.prevent_default();
                s.camera.nudge_dolly(1.0 + (e.delta_y() as f32) * 0.0006);
            }
        }) as Box<dyn FnMut(web_sys::WheelEvent)>);
        let wopts = web_sys::AddEventListenerOptions::new();
        wopts.set_passive(false);
        let _ = window().add_event_listener_with_callback_and_add_event_listener_options(
            "wheel",
            on_wheel.as_ref().unchecked_ref(),
            &wopts,
        );

        Handle { inner, raf, on_move: Some(on_move), on_wheel: Some(on_wheel) }
    }

    pub fn stop(&mut self) {
        self.inner.borrow_mut().running = false;
        let win = window();
        if let Some(c) = self.on_move.take() {
            let _ = win.remove_event_listener_with_callback("pointermove", c.as_ref().unchecked_ref());
        }
        if let Some(c) = self.on_wheel.take() {
            let _ = win.remove_event_listener_with_callback("wheel", c.as_ref().unchecked_ref());
        }
        self.raf.borrow_mut().take();
    }

    pub fn set_params(&self, nodes: usize, clusters: usize, density: f32) {
        let mut s = self.inner.borrow_mut();
        let g = Graph::generate(nodes, clusters, density, s.seed);
        s.scene.set_graph(&g);
        s.nodes = g.nodes;
        s.edges = g.edge_count();
        s.clusters = clusters;
        s.density = density;
    }

    /// Camera props from the page. Defaults here are only a starting point;
    /// the component's declared defaults differ and must be passed in.
    pub fn set_camera(&self, zoom: f32, drift_speed: f32, parallax: f32, mass: f32) {
        self.inner.borrow_mut().camera.set_props(zoom, drift_speed, parallax, mass);
    }

    pub fn set_tilting(&self, on: bool) { self.inner.borrow_mut().camera.tilting = on; }
    pub fn set_simulate(&self, on: bool) { self.inner.borrow_mut().simulate = on; }
    pub fn set_pointer(&self, x: f32, y: f32) {
        let mut s = self.inner.borrow_mut();
        s.camera.mx = x;
        s.camera.my = y;
    }
    pub fn nodes(&self) -> usize { self.inner.borrow().nodes }
    pub fn edges(&self) -> usize { self.inner.borrow().edges }
}
