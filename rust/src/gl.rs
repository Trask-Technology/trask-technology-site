//! WebGL2 renderer, via `glow`.
//!
//! This exists because WebGL2 has one thing WebGPU does not: point sprites.
//! `gl_PointSize` + `gl_PointCoord` turn a single vertex into a round dot, so a
//! 200k-node graph costs 200k vertices here against 1.2M for the quad expansion
//! WGSL forces on the wgpu path. The GLSL below is the page's, unchanged.

use crate::graph::Graph;
use glow::HasContext;

const VS: &str = r#"#version 300 es
in vec3 aPos; uniform mat3 uRot; uniform float uScale; uniform vec2 uFix; uniform float uPt;
out float vD;
void main(){
  vec3 p = uRot * aPos;
  float persp = 1.0 / (2.0 - p.z * 0.45);
  vD = p.z;
  gl_Position = vec4(p.xy * uScale * persp * uFix, 0.0, 1.0);
  gl_PointSize = uPt * (0.75 + 0.6 * (p.z + 1.0));
}"#;

const FS_POINT: &str = r#"#version 300 es
precision mediump float; in float vD; uniform float uA; out vec4 o;
void main(){
  vec2 d = gl_PointCoord - 0.5;
  if (dot(d, d) > 0.25) discard;
  float f = 0.28 + 0.72 * smoothstep(-1.1, 1.0, vD);
  o = vec4(1.0, 1.0, 1.0, uA * f);
}"#;

const FS_LINE: &str = r#"#version 300 es
precision mediump float; in float vD; uniform float uA; out vec4 o;
void main(){ float f = 0.2 + 0.8 * smoothstep(-1.1, 1.0, vD); o = vec4(0.86, 0.93, 0.96, uA * f); }"#;

struct Prog {
    program: glow::Program,
    vao: glow::VertexArray,
    u_rot: Option<glow::UniformLocation>,
    u_scale: Option<glow::UniformLocation>,
    u_fix: Option<glow::UniformLocation>,
    u_pt: Option<glow::UniformLocation>,
    u_a: Option<glow::UniformLocation>,
}

pub struct Renderer {
    gl: glow::Context,
    point: Prog,
    line: Prog,
    vbo: glow::Buffer,
    ebo: glow::Buffer,
    nodes: i32,
    edge_indices: i32,
    width: u32,
    height: u32,
}

unsafe fn compile(gl: &glow::Context, vsrc: &str, fsrc: &str) -> Result<glow::Program, String> {
    let program = gl.create_program()?;
    for (kind, src) in [(glow::VERTEX_SHADER, vsrc), (glow::FRAGMENT_SHADER, fsrc)] {
        let sh = gl.create_shader(kind)?;
        gl.shader_source(sh, src);
        gl.compile_shader(sh);
        if !gl.get_shader_compile_status(sh) {
            return Err(gl.get_shader_info_log(sh));
        }
        gl.attach_shader(program, sh);
        gl.delete_shader(sh);
    }
    gl.link_program(program);
    if !gl.get_program_link_status(program) {
        return Err(gl.get_program_info_log(program));
    }
    Ok(program)
}

impl Renderer {
    pub fn new(canvas: &web_sys::HtmlCanvasElement, graph: &Graph) -> Result<Renderer, String> {
        use wasm_bindgen::JsCast;
        // The same attributes the page's JS path requests. The defaults are all
        // wrong for this: antialias defaults to TRUE, and MSAA both softens 1px
        // lines and point sprites — making them look larger — and costs real
        // fill rate at 5K. alpha defaults to TRUE, which would give the canvas
        // its own alpha channel to composite through the wrapper's CSS mask on
        // top of the fade that mask already applies.
        let opts = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&opts, &"antialias".into(), &wasm_bindgen::JsValue::FALSE);
        let _ = js_sys::Reflect::set(&opts, &"alpha".into(), &wasm_bindgen::JsValue::FALSE);
        let _ = js_sys::Reflect::set(&opts, &"powerPreference".into(), &"high-performance".into());
        let ctx = canvas
            .get_context_with_context_options("webgl2", &opts)
            .map_err(|_| "getContext threw".to_string())?
            .ok_or("no webgl2 context")?
            .dyn_into::<web_sys::WebGl2RenderingContext>()
            .map_err(|_| "not a WebGL2 context".to_string())?;
        let gl = glow::Context::from_webgl2_context(ctx);

        unsafe {
            let vbo = gl.create_buffer()?;
            let ebo = gl.create_buffer()?;

            let mk = |fsrc: &str, with_index: bool| -> Result<Prog, String> {
                let program = compile(&gl, VS, fsrc)?;
                let vao = gl.create_vertex_array()?;
                gl.bind_vertex_array(Some(vao));
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
                let loc = gl.get_attrib_location(program, "aPos").unwrap_or(0);
                gl.enable_vertex_attrib_array(loc);
                // Tightly packed vec3, matching the JS path. Reading xyz out of
                // the generator's vec4 at stride 16 would avoid a repack, but it
                // ships 33% more vertex data and spreads each edge's random
                // index fetch over more cache lines — and edges are the bulk of
                // the vertex work.
                gl.vertex_attrib_pointer_f32(loc, 3, glow::FLOAT, false, 0, 0);
                if with_index {
                    gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));
                }
                gl.bind_vertex_array(None);
                Ok(Prog {
                    u_rot: gl.get_uniform_location(program, "uRot"),
                    u_scale: gl.get_uniform_location(program, "uScale"),
                    u_fix: gl.get_uniform_location(program, "uFix"),
                    u_pt: gl.get_uniform_location(program, "uPt"),
                    u_a: gl.get_uniform_location(program, "uA"),
                    program,
                    vao,
                })
            };
            let point = mk(FS_POINT, false)?;
            let line = mk(FS_LINE, true)?;

            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            gl.clear_color(0.0824, 0.3098, 0.4118, 1.0);

            let mut r = Renderer {
                gl,
                point,
                line,
                vbo,
                ebo,
                nodes: 0,
                edge_indices: 0,
                width: 1,
                height: 1,
            };
            r.set_graph(graph);
            Ok(r)
        }
    }

    pub fn set_graph(&mut self, graph: &Graph) {
        unsafe {
            let gl = &self.gl;
            // vec4 (xyz + community index) down to vec3: the community index is
            // only needed by the compute layout, which WebGL2 cannot run anyway.
            let mut xyz: Vec<f32> = Vec::with_capacity(graph.nodes * 3);
            for i in 0..graph.nodes {
                xyz.push(graph.pos[i * 4]);
                xyz.push(graph.pos[i * 4 + 1]);
                xyz.push(graph.pos[i * 4 + 2]);
            }
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes_of_f32(&xyz), glow::STATIC_DRAW);
            gl.bind_vertex_array(Some(self.line.vao));
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.ebo));
            gl.buffer_data_u8_slice(
                glow::ELEMENT_ARRAY_BUFFER,
                bytes_of_u32(&graph.edges),
                glow::STATIC_DRAW,
            );
            gl.bind_vertex_array(None);
        }
        self.nodes = graph.nodes as i32;
        self.edge_indices = graph.edges.len() as i32;
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 || (width == self.width && height == self.height) {
            return;
        }
        self.width = width;
        self.height = height;
        unsafe { self.gl.viewport(0, 0, width as i32, height as i32) };
    }

    pub fn frame(&mut self, rot: [f32; 9], scale: f32, point_px: f32) {
        let aspect = self.width as f32 / self.height.max(1) as f32;
        let fix = [(1.0 / aspect).min(1.0), aspect.min(1.0)];
        unsafe {
            let gl = &self.gl;
            gl.clear(glow::COLOR_BUFFER_BIT);

            // edges first, then nodes over them — same order and alphas as wgpu
            for (p, alpha, is_nodes) in
                [(&self.line, 0.035f32, false), (&self.point, 0.5f32, true)]
            {
                gl.use_program(Some(p.program));
                gl.bind_vertex_array(Some(p.vao));
                gl.uniform_matrix_3_f32_slice(p.u_rot.as_ref(), false, &rot);
                gl.uniform_1_f32(p.u_scale.as_ref(), scale);
                gl.uniform_2_f32_slice(p.u_fix.as_ref(), &fix);
                gl.uniform_1_f32(p.u_pt.as_ref(), point_px);
                gl.uniform_1_f32(p.u_a.as_ref(), alpha);
                if is_nodes {
                    gl.draw_arrays(glow::POINTS, 0, self.nodes);
                } else {
                    gl.draw_elements(glow::LINES, self.edge_indices, glow::UNSIGNED_INT, 0);
                }
            }
            gl.bind_vertex_array(None);
        }
    }
}

fn bytes_of_f32(v: &[f32]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}
fn bytes_of_u32(v: &[u32]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}
