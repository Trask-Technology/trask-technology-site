//! wgpu renderer: owns the device, buffers, compute pass and render pass.
//! WebGPU only — the layout pass is a compute shader, which WebGL2 cannot run,
//! so a browser without WebGPU gets no adapter here and the page falls back to
//! the JS path.

use crate::graph::Graph;
use bytemuck::{Pod, Zeroable};
use std::borrow::Cow;
use wgpu::util::DeviceExt;

const WG: u32 = 64;

/// Indices into the per-pass draw uniform/bind-group arrays. Edges draw first
/// (they carry the clear), nodes draw over them.
const DRAW_EDGES: usize = 0;
const DRAW_NODES: usize = 1;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DrawUniform {
    rot: [[f32; 4]; 4],
    fix: [f32; 2],
    scale: f32,
    alpha: f32,
    pt: [f32; 2],
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SimUniform {
    n: u32,
    clusters: u32,
    dt: f32,
    spring: f32,
    repel: f32,
    damp: f32,
    max_deg: u32,
    _pad: u32,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,

    node_pipeline: wgpu::RenderPipeline,
    edge_pipeline: wgpu::RenderPipeline,
    sim_pipeline: wgpu::ComputePipeline,

    draw_bind: [wgpu::BindGroup; 2],
    sim_bind: wgpu::BindGroup,

    draw_uniform: [wgpu::Buffer; 2],
    sim_uniform: wgpu::Buffer,
    quad: wgpu::Buffer,
    pos: wgpu::Buffer,
    idx: wgpu::Buffer,

    nodes: u32,
    edge_indices: u32,
    clusters: u32,
    groups: u32,
}

impl Renderer {
    pub async fn new(
        target: wgpu::SurfaceTarget<'static>,
        width: u32,
        height: u32,
        graph: &Graph,
    ) -> Result<Renderer, String> {
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(target).map_err(|e| e.to_string())?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or("no adapter")?;

        // The sim pass needs compute and six storage bindings; the WebGL2
        // downlevel profile zeroes both, so ask for what the adapter actually has.
        let limits = adapter.limits();
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("trask"),
                    required_features: wgpu::Features::empty(),
                    required_limits: limits,
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await
            .map_err(|e| e.to_string())?;

        let caps = surface.get_capabilities(&adapter);
        // Clear/blend colours below are authored in sRGB byte space, so pick a
        // non-sRGB target and skip the automatic linear conversion.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let draw_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("draw"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shaders/draw.wgsl"))),
        });
        let sim_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sim"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shaders/sim.wgsl"))),
        });

        // ---- buffers ------------------------------------------------------
        let pos = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("pos"),
            contents: bytemuck::cast_slice(&graph.pos),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        let vel = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vel"),
            size: (graph.nodes * 16) as u64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let rest = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rest"),
            contents: bytemuck::cast_slice(&graph.pos),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let off = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("csr_off"),
            contents: bytemuck::cast_slice(&graph.csr_off),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let nbr = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("csr_nbr"),
            contents: bytemuck::cast_slice(&graph.csr_nbr),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let cen = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("centroids"),
            contents: bytemuck::cast_slice(&graph.centroids),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let idx = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("edges"),
            contents: bytemuck::cast_slice(&graph.edges),
            usage: wgpu::BufferUsages::INDEX,
        });
        let quad: [f32; 12] = [-0.5, -0.5, 0.5, -0.5, 0.5, 0.5, -0.5, -0.5, 0.5, 0.5, -0.5, 0.5];
        let quad = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad"),
            contents: bytemuck::cast_slice(&quad),
            usage: wgpu::BufferUsages::VERTEX,
        });
        // One uniform buffer per draw pass. `write_buffer` is staged to the start
        // of the next submission, so a single buffer written twice would hand both
        // passes whichever value was written last.
        let draw_uniform = [DRAW_EDGES, DRAW_NODES].map(|i| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(if i == DRAW_EDGES { "draw_u_edges" } else { "draw_u_nodes" }),
                size: std::mem::size_of::<DrawUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let sim_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sim_u"),
            size: std::mem::size_of::<SimUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // ---- pipelines ----------------------------------------------------
        let blend = Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        });
        let target = [Some(wgpu::ColorTargetState {
            format,
            blend,
            write_mask: wgpu::ColorWrites::ALL,
        })];

        // Explicit, shared layout: both draw pipelines bind the same uniform, and
        // one bind group has to be valid against either pipeline.
        let draw_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("draw_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let draw_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("draw_layout"),
            bind_group_layouts: &[&draw_bgl],
            push_constant_ranges: &[],
        });

        let node_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nodes"),
            layout: Some(&draw_layout),
            vertex: wgpu::VertexState {
                module: &draw_shader,
                entry_point: "vs_node",
                compilation_options: Default::default(),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: 8,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: 16,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &wgpu::vertex_attr_array![1 => Float32x3],
                    },
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &draw_shader,
                entry_point: "fs_node",
                compilation_options: Default::default(),
                targets: &target,
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });

        let edge_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("edges"),
            layout: Some(&draw_layout),
            vertex: wgpu::VertexState {
                module: &draw_shader,
                entry_point: "vs_edge",
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &draw_shader,
                entry_point: "fs_edge",
                compilation_options: Default::default(),
                targets: &target,
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });

        let sim_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("sim"),
            layout: None,
            module: &sim_shader,
            entry_point: "step",
            compilation_options: Default::default(),
            cache: None,
        });

        let draw_bind = [DRAW_EDGES, DRAW_NODES].map(|i| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("draw_bind"),
                layout: &draw_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: draw_uniform[i].as_entire_binding(),
                }],
            })
        });

        let sim_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sim_bind"),
            layout: &sim_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: pos.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: vel.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: off.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: nbr.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: cen.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: sim_uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 6, resource: rest.as_entire_binding() },
            ],
        });

        Ok(Renderer {
            device,
            queue,
            surface,
            config,
            node_pipeline,
            edge_pipeline,
            sim_pipeline,
            draw_bind,
            sim_bind,
            draw_uniform,
            sim_uniform,
            quad,
            pos,
            idx,
            nodes: graph.nodes as u32,
            edge_indices: graph.edges.len() as u32,
            clusters: graph.clusters as u32,
            groups: (graph.nodes as u32 + WG - 1) / WG,
        })
    }

    /// Swap in a freshly generated graph without tearing down the device.
    /// Pipelines, layouts and the draw bind groups are size-independent, so only
    /// the storage buffers and the sim bind group have to be rebuilt.
    pub fn set_graph(&mut self, graph: &Graph) {
        let pos = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("pos"),
            contents: bytemuck::cast_slice(&graph.pos),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        let vel = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vel"),
            size: (graph.nodes * 16) as u64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let rest = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rest"),
            contents: bytemuck::cast_slice(&graph.pos),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let off = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("csr_off"),
            contents: bytemuck::cast_slice(&graph.csr_off),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let nbr = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("csr_nbr"),
            contents: bytemuck::cast_slice(&graph.csr_nbr),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let cen = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("centroids"),
            contents: bytemuck::cast_slice(&graph.centroids),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let idx = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("edges"),
            contents: bytemuck::cast_slice(&graph.edges),
            usage: wgpu::BufferUsages::INDEX,
        });

        self.sim_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sim_bind"),
            layout: &self.sim_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: pos.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: vel.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: off.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: nbr.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: cen.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: self.sim_uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 6, resource: rest.as_entire_binding() },
            ],
        });

        self.pos = pos;
        self.idx = idx;
        self.nodes = graph.nodes as u32;
        self.edge_indices = graph.edges.len() as u32;
        self.clusters = graph.clusters as u32;
        self.groups = (graph.nodes as u32 + WG - 1) / WG;
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 || (width == self.config.width && height == self.config.height) {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// `rot` is a column-major 3x3 expanded to 4x4 by the caller's convention.
    pub fn frame(&mut self, rot: [f32; 9], scale: f32, point_px: f32, simulate: bool, dt: f32) {
        let aspect = self.config.width as f32 / self.config.height.max(1) as f32;
        let fix = [(1.0 / aspect).min(1.0), aspect.min(1.0)];
        let rot4 = [
            [rot[0], rot[1], rot[2], 0.0],
            [rot[3], rot[4], rot[5], 0.0],
            [rot[6], rot[7], rot[8], 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];

        let ptx = point_px * 2.0 / self.config.width as f32;
        let pty = point_px * 2.0 / self.config.height as f32;

        self.queue.write_buffer(
            &self.sim_uniform,
            0,
            bytemuck::bytes_of(&SimUniform {
                n: self.nodes,
                clusters: self.clusters,
                dt,
                spring: 0.6,
                repel: 0.02,
                damp: 0.9,
                max_deg: 24,
                _pad: 0,
            }),
        );

        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(_) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });

        if simulate {
            let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("layout"),
                timestamp_writes: None,
            });
            cp.set_pipeline(&self.sim_pipeline);
            cp.set_bind_group(0, &self.sim_bind, &[]);
            cp.dispatch_workgroups(self.groups, 1, 1);
        }

        // Both passes' uniforms are staged before the submission either way, so
        // write them up front and keep the drawing in ONE render pass. Splitting
        // it in two costs a full store + reload of the colour attachment between
        // them — pure bandwidth, scaling with pixel count, which at 5K dominates
        // everything else in the frame.
        for (slot, alpha) in [(DRAW_EDGES, 0.035f32), (DRAW_NODES, 0.5f32)] {
            self.queue.write_buffer(
                &self.draw_uniform[slot],
                0,
                bytemuck::bytes_of(&DrawUniform {
                    rot: rot4,
                    fix,
                    scale,
                    alpha,
                    pt: [ptx, pty],
                    _pad: [0.0; 2],
                }),
            );
        }

        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("draw"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.0824, g: 0.3098, b: 0.4118, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // edges first, nodes over them — same order as before, one pass
            pass.set_pipeline(&self.edge_pipeline);
            pass.set_bind_group(0, &self.draw_bind[DRAW_EDGES], &[]);
            pass.set_vertex_buffer(0, self.pos.slice(..));
            pass.set_index_buffer(self.idx.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..self.edge_indices, 0, 0..1);

            pass.set_pipeline(&self.node_pipeline);
            pass.set_bind_group(0, &self.draw_bind[DRAW_NODES], &[]);
            pass.set_vertex_buffer(0, self.quad.slice(..));
            pass.set_vertex_buffer(1, self.pos.slice(..));
            pass.draw(0..6, 0..self.nodes);
        }

        self.queue.submit(Some(enc.finish()));
        frame.present();
    }
}
