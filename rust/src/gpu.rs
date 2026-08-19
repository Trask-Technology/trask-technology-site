//! wgpu renderer: owns the device, buffers, compute pass and render pass.
//! The same code compiles native (wgpu picks Vulkan/Metal/DX12) and to wasm
//! (WebGPU), so the browser hero and a desktop tool share one pipeline.

use crate::graph::Graph;
use bytemuck::{Pod, Zeroable};
use std::borrow::Cow;
use wgpu::util::DeviceExt;

const WG: u32 = 64;

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

    draw_bind: wgpu::BindGroup,
    sim_bind: wgpu::BindGroup,

    draw_uniform: wgpu::Buffer,
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
        target: wgpu::SurfaceTargetUnsafe,
        width: u32,
        height: u32,
        graph: &Graph,
    ) -> Result<Renderer, String> {
        let instance = wgpu::Instance::default();
        // SAFETY: the canvas outlives the renderer — the JS wrapper holds both.
        let surface = unsafe { instance.create_surface_unsafe(target) }.map_err(|e| e.to_string())?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or("no adapter")?;

        let limits = wgpu::Limits {
            max_storage_buffer_binding_size: adapter.limits().max_storage_buffer_binding_size,
            max_buffer_size: adapter.limits().max_buffer_size,
            ..wgpu::Limits::downlevel_webgl2_defaults()
        };
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
        let format = caps.formats[0];
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
        let draw_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("draw_u"),
            size: std::mem::size_of::<DrawUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
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

        let node_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nodes"),
            layout: None,
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
            layout: None,
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

        let draw_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("draw_bind"),
            layout: &node_pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: draw_uniform.as_entire_binding(),
            }],
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

        // edges then nodes, each with its own alpha
        for (pipeline, alpha, is_nodes) in [
            (&self.edge_pipeline, 0.035f32, false),
            (&self.node_pipeline, 0.5f32, true),
        ] {
            self.queue.write_buffer(
                &self.draw_uniform,
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
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("draw"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if is_nodes {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(wgpu::Color { r: 0.0824, g: 0.3098, b: 0.4118, a: 1.0 })
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.draw_bind, &[]);
            if is_nodes {
                pass.set_vertex_buffer(0, self.quad.slice(..));
                pass.set_vertex_buffer(1, self.pos.slice(..));
                pass.draw(0..6, 0..self.nodes);
            } else {
                pass.set_vertex_buffer(0, self.pos.slice(..));
                pass.set_index_buffer(self.idx.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..self.edge_indices, 0, 0..1);
            }
        }

        self.queue.submit(Some(enc.finish()));
        frame.present();
    }
}
