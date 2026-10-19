//! wgpu renderer for the 3D viewport.
//!
//! Renders into an offscreen colour texture (with MSAA and a depth buffer of
//! its own) which egui then displays as an ordinary image. This keeps the 3D
//! pass independent of egui's render pass setup: we control depth, sample
//! count and clear colour fully.

use eframe::egui;
use eframe::egui_wgpu::RenderState;
use eframe::wgpu;
use eframe::wgpu::util::DeviceExt;
use nalgebra::Matrix4;

use crate::kernel::{Point3, Real, Vector3};

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// position (3) + normal (3) + colour (3), all `f32`.
const VERTEX_STRIDE: u64 = 9 * 4;
/// Transparent faces: position (3) + normal (3) + colour with alpha (4).
const ALPHA_STRIDE: u64 = 10 * 4;
/// Background (gamma-encoded, written straight into the target).
const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.90,
    g: 0.91,
    b: 0.93,
    a: 1.0,
};

/// CPU-side geometry for one upload: triangles and line segments.
#[derive(Default)]
pub struct SceneGeometry {
    tri_vertices: Vec<u8>,
    tri_count: u32,
    tri_indices: Vec<u32>,
    line_vertices: Vec<u8>,
    line_vertex_count: u32,
    alpha_vertices: Vec<u8>,
    alpha_count: u32,
    alpha_indices: Vec<u32>,
}

impl SceneGeometry {
    /// Index counts of the opaque and the transparent triangles.
    #[cfg(test)]
    pub fn counts(&self) -> (usize, usize) {
        (self.tri_indices.len(), self.alpha_indices.len())
    }

    /// Adds triangles with a colour per vertex (`colors[i]` for
    /// `positions[i]`); only the vertices the triangles use are kept.
    pub fn add_mesh_colored(
        &mut self,
        positions: &[Point3],
        normals: &[Vector3],
        triangles: &[[u32; 3]],
        colors: &[[f32; 3]],
    ) {
        let base = self.tri_count;
        for ((p, n), c) in positions.iter().zip(normals).zip(colors) {
            push_vertex(&mut self.tri_vertices, p, n, *c);
            self.tri_count += 1;
        }
        self.tri_indices
            .extend(triangles.iter().flatten().map(|i| base + i));
    }

    /// Adds see-through triangles (drawn after everything else, blended,
    /// without depth writes, both sides).
    pub fn add_mesh_transparent(
        &mut self,
        positions: &[Point3],
        normals: &[Vector3],
        triangles: &[[u32; 3]],
        color: [f32; 4],
    ) {
        let base = self.alpha_count;
        for (p, n) in positions.iter().zip(normals) {
            for v in [
                p.x as f32, p.y as f32, p.z as f32, n.x as f32, n.y as f32, n.z as f32, color[0],
                color[1], color[2], color[3],
            ] {
                self.alpha_vertices.extend_from_slice(&v.to_le_bytes());
            }
        }
        self.alpha_count += positions.len() as u32;
        self.alpha_indices
            .extend(triangles.iter().flatten().map(|i| base + i));
    }

    /// Adds a triangle mesh (positions, normals, CCW triangles) in one colour.
    pub fn add_mesh(
        &mut self,
        positions: &[Point3],
        normals: &[Vector3],
        triangles: &[[u32; 3]],
        color: [f32; 3],
    ) {
        let base = self.tri_count;
        for (p, n) in positions.iter().zip(normals) {
            push_vertex(&mut self.tri_vertices, p, n, color);
        }
        self.tri_count += positions.len() as u32;
        self.tri_indices
            .extend(triangles.iter().flatten().map(|i| base + i));
    }

    /// Adds a line segment.
    pub fn add_line(&mut self, a: &Point3, b: &Point3, color: [f32; 3]) {
        for p in [a, b] {
            push_vertex(&mut self.line_vertices, p, &Vector3::zeros(), color);
        }
        self.line_vertex_count += 2;
    }

    /// Adds a polyline as consecutive segments.
    pub fn add_polyline(&mut self, pts: &[Point3], color: [f32; 3]) {
        for w in pts.windows(2) {
            self.add_line(&w[0], &w[1], color);
        }
    }
}

fn push_vertex(buf: &mut Vec<u8>, p: &Point3, n: &Vector3, c: [f32; 3]) {
    for v in [
        p.x as f32, p.y as f32, p.z as f32, n.x as f32, n.y as f32, n.z as f32, c[0], c[1], c[2],
    ] {
        buf.extend_from_slice(&v.to_le_bytes());
    }
}

/// Geometry resident on the GPU.
struct GpuGeometry {
    tri_vbuf: Option<wgpu::Buffer>,
    tri_ibuf: Option<wgpu::Buffer>,
    tri_index_count: u32,
    line_vbuf: Option<wgpu::Buffer>,
    line_vertex_count: u32,
    alpha_vbuf: Option<wgpu::Buffer>,
    alpha_ibuf: Option<wgpu::Buffer>,
    alpha_index_count: u32,
}

/// Offscreen render target, recreated when the viewport is resized.
struct Target {
    size: [u32; 2],
    msaa_view: Option<wgpu::TextureView>,
    color_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
}

/// Owns all GPU resources of the 3D viewport.
pub struct ViewportRenderer {
    shaded: wgpu::RenderPipeline,
    backfaces: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    transparent: wgpu::RenderPipeline,
    uniform_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    sample_count: u32,
    geometry: Option<GpuGeometry>,
    target: Option<Target>,
    texture_id: Option<egui::TextureId>,
}

impl ViewportRenderer {
    /// Creates pipelines and buffers on egui's wgpu device.
    pub fn new(rs: &RenderState) -> Self {
        let device = &rs.device;

        let msaa_ok = |f: wgpu::TextureFormat| {
            rs.adapter
                .get_texture_format_features(f)
                .flags
                .sample_count_supported(4)
        };
        let sample_count = if msaa_ok(COLOR_FORMAT) && msaa_ok(DEPTH_FORMAT) {
            4
        } else {
            1
        };

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewport shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("viewport.wgsl").into()),
        });

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport uniforms"),
            size: 96, // mat4x4<f32> + 2 × vec4<f32>
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewport bgl"),
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
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewport bind group"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3];
        let pipeline = |label, topology, cull_mode, fs: &str, bias: wgpu::DepthBiasState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: VERTEX_STRIDE,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attributes,
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    topology,
                    front_face: wgpu::FrontFace::Ccw, // kernel triangles are CCW from outside
                    cull_mode,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias,
                }),
                multisample: wgpu::MultisampleState {
                    count: sample_count,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        // Push faces slightly back so edges drawn on them win the depth test,
        // and back faces much further back so that, where they meet front
        // faces at a silhouette, the front face always wins.
        let bias = |constant| wgpu::DepthBiasState {
            constant,
            slope_scale: 1.5 * constant as f32 / 2.0,
            clamp: 0.0,
        };
        use wgpu::{Face, PrimitiveTopology as Topo};
        let shaded = pipeline(
            "front faces",
            Topo::TriangleList,
            Some(Face::Back),
            "fs_shaded",
            bias(2),
        );
        let backfaces = pipeline(
            "back faces",
            Topo::TriangleList,
            Some(Face::Front),
            "fs_backface",
            bias(16),
        );
        let lines = pipeline("lines", Topo::LineList, None, "fs_flat", Default::default());
        // See-through faces: blended over the opaque scene, depth-tested
        // but not written (faces behind stay visible), both sides.
        let alpha_attributes =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4];
        let transparent = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("transparent faces"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_alpha"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: ALPHA_STRIDE,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &alpha_attributes,
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: Topo::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: bias(2),
            }),
            multisample: wgpu::MultisampleState {
                count: sample_count,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_alpha"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        Self {
            shaded,
            backfaces,
            lines,
            transparent,
            uniform_buf,
            bind_group,
            sample_count,
            geometry: None,
            target: None,
            texture_id: None,
        }
    }

    /// Replaces the displayed geometry.
    pub fn upload(&mut self, rs: &RenderState, g: &SceneGeometry) {
        let buffer = |label, bytes: &[u8], usage| {
            (!bytes.is_empty()).then(|| {
                rs.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(label),
                        contents: bytes,
                        usage,
                    })
            })
        };
        let index_bytes: Vec<u8> = g.tri_indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let alpha_index_bytes: Vec<u8> = g
            .alpha_indices
            .iter()
            .flat_map(|i| i.to_le_bytes())
            .collect();
        self.geometry = Some(GpuGeometry {
            tri_vbuf: buffer("tri vertices", &g.tri_vertices, wgpu::BufferUsages::VERTEX),
            tri_ibuf: buffer("tri indices", &index_bytes, wgpu::BufferUsages::INDEX),
            tri_index_count: g.tri_indices.len() as u32,
            line_vbuf: buffer(
                "line vertices",
                &g.line_vertices,
                wgpu::BufferUsages::VERTEX,
            ),
            line_vertex_count: g.line_vertex_count,
            alpha_vbuf: buffer(
                "alpha vertices",
                &g.alpha_vertices,
                wgpu::BufferUsages::VERTEX,
            ),
            alpha_ibuf: buffer(
                "alpha indices",
                &alpha_index_bytes,
                wgpu::BufferUsages::INDEX,
            ),
            alpha_index_count: g.alpha_indices.len() as u32,
        });
    }

    /// Renders the scene at `size` physical pixels and returns the egui
    /// texture to display.
    pub fn render(
        &mut self,
        rs: &RenderState,
        size: [u32; 2],
        view_proj: &Matrix4<Real>,
        eye: &Point3,
        clip: Option<(Vector3, Real)>,
    ) -> egui::TextureId {
        let size = [size[0].max(1), size[1].max(1)];
        self.ensure_target(rs, size);
        let Some(target) = self.target.as_ref() else {
            return egui::TextureId::default();
        };

        let mut uniforms = Vec::with_capacity(96);
        let (cn, cw) = clip.unwrap_or((Vector3::zeros(), 0.0));
        for v in view_proj
            .iter()
            .copied()
            .chain([eye.x, eye.y, eye.z, 1.0])
            .chain([cn.x, cn.y, cn.z, cw])
        {
            uniforms.extend_from_slice(&(v as f32).to_le_bytes()); // column-major, as WGSL expects
        }
        rs.queue.write_buffer(&self.uniform_buf, 0, &uniforms);

        let mut encoder = rs
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("viewport encoder"),
            });
        {
            let (view, resolve_target) = match &target.msaa_view {
                Some(msaa) => (msaa, Some(&target.color_view)),
                None => (&target.color_view, None),
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(CLEAR),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.bind_group, &[]);

            if let Some(g) = &self.geometry {
                if let (Some(vb), Some(ib)) = (&g.tri_vbuf, &g.tri_ibuf) {
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                    for pipeline in [&self.shaded, &self.backfaces] {
                        pass.set_pipeline(pipeline);
                        pass.draw_indexed(0..g.tri_index_count, 0, 0..1);
                    }
                }
                if let Some(vb) = &g.line_vbuf {
                    pass.set_pipeline(&self.lines);
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.draw(0..g.line_vertex_count, 0..1);
                }
                // Transparent faces last, over everything opaque.
                if let (Some(vb), Some(ib)) = (&g.alpha_vbuf, &g.alpha_ibuf) {
                    pass.set_pipeline(&self.transparent);
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..g.alpha_index_count, 0, 0..1);
                }
            }
        }
        // Submitted before egui's own frame submission, so the texture is
        // ready when egui samples it later this frame.
        rs.queue.submit(Some(encoder.finish()));

        self.texture_id.unwrap_or_default()
    }

    /// (Re)creates the offscreen textures when the size changes and keeps
    /// the egui texture registration pointing at the current colour view.
    fn ensure_target(&mut self, rs: &RenderState, size: [u32; 2]) {
        if self.target.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let device = &rs.device;
        let texture = |label, format, samples, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let color_view = texture(
            "viewport color",
            COLOR_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let msaa_view = (self.sample_count > 1).then(|| {
            texture(
                "viewport msaa",
                COLOR_FORMAT,
                self.sample_count,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            )
        });
        let depth_view = texture(
            "viewport depth",
            DEPTH_FORMAT,
            self.sample_count,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );

        let mut renderer = rs.renderer.write();
        match self.texture_id {
            Some(id) => renderer.update_egui_texture_from_wgpu_texture(
                device,
                &color_view,
                wgpu::FilterMode::Linear,
                id,
            ),
            None => {
                self.texture_id = Some(renderer.register_native_texture(
                    device,
                    &color_view,
                    wgpu::FilterMode::Linear,
                ))
            }
        }
        self.target = Some(Target {
            size,
            msaa_view,
            color_view,
            depth_view,
        });
    }
}
