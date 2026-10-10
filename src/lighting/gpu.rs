//! The render-world half of lighting: buffers, and the compute dispatches.
//!
//! The main world's [`LightUpload`] is taken whole in `ExtractSchedule`, so
//! each change crosses over exactly once; dirty chunks then wait here until
//! the pipelines have compiled and the lightmap image exists on the GPU, which
//! is the first few frames of a screen and never again.
//!
//! Two kinds of work, in one compute pass:
//!
//! * **the sky** (`assets/shaders/sky.wgsl`) — once per scene, before its
//!   first bake: [`SKY_ITERATIONS`] propagation passes over the whole map,
//!   ping-ponging two buffers, leaving the sky field the bake reads. It
//!   depends on the walls and the ceiling alone, so nothing re-runs it.
//! * **the bake** (`assets/shaders/lighting.wgsl`) — the dirty chunks only:
//!   the lights' RGB, and the sky factor into alpha.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bevy::prelude::*;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{
    storage_buffer_read_only_sized, storage_buffer_sized, texture_storage_2d, uniform_buffer_sized,
};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderGraph, RenderGraphSystems};
use bevy::render::texture::GpuImage;
use bevy::render::{ExtractSchedule, MainWorld, RenderApp, RenderStartup};

use super::scene::{CHUNK, SKY_ITERATIONS, SKY_STEP, SKY_STEP_DIAGONAL, SUBTILES, WORKGROUP};
use super::LightUpload;

/// Everything about a scene that does not change while it is shown.
pub struct StaticUpload {
    pub image: Handle<Image>,
    pub baked: Arc<AtomicU64>,
    pub size: UVec2,
    pub chunks_x: u32,
    pub map_width: u32,
    pub occlusion: Vec<u32>,
    pub ceiling: Vec<u32>,
    pub chunk_offsets: Vec<u32>,
    pub chunk_lights: Vec<u32>,
}

pub(super) fn build(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .init_resource::<LightBake>()
        .add_systems(RenderStartup, init_pipelines)
        .add_systems(ExtractSchedule, extract_upload)
        // In `Render`, ahead of every camera: what a camera draws this frame
        // is lit by what was baked this frame.
        .add_systems(
            RenderGraph,
            bake.in_set(RenderGraphSystems::Render)
                .before(bevy::core_pipeline::schedule::camera_driver),
        );
}

/// The render world's copy of the scene, and what is waiting to be baked.
#[derive(Resource, Default)]
struct LightBake {
    scene: Option<SceneBuffers>,
    /// Pending, indexed by chunk: a chunk dirtied twice before it is baked
    /// is baked once.
    pending: Vec<bool>,
    pending_any: bool,
    generation: u64,
}

struct SceneBuffers {
    image: Handle<Image>,
    baked: Arc<AtomicU64>,
    size: UVec2,
    fixed: Option<Fixed>,
    lights: Option<Buffer>,
    /// Whether the sky field has been propagated into `fixed.sky[0]`.
    sky_done: bool,
    /// Kept to create the buffers in the render system, where the device is.
    upload: Option<StaticUpload>,
    lights_upload: Option<Vec<f32>>,
}

/// The buffers made once per scene.
struct Fixed {
    params: Buffer,
    sky_params: Buffer,
    occlusion: Buffer,
    ceiling: Buffer,
    chunk_offsets: Buffer,
    chunk_lights: Buffer,
    /// Ping and pong; the finished field is always in `sky[0]`.
    sky: [Buffer; 2],
}

fn extract_upload(mut main: ResMut<MainWorld>, mut bake: ResMut<LightBake>) {
    let Some(mut upload) = main.get_resource_mut::<LightUpload>() else {
        return;
    };
    let upload = std::mem::take(&mut *upload);
    if upload.clear {
        *bake = LightBake::default();
    }
    if let Some(scene) = upload.scene {
        let chunks = scene.chunk_offsets.len() - 1;
        bake.pending = vec![false; chunks];
        bake.scene = Some(SceneBuffers::pending(scene));
    }
    let Some(buffers) = bake.scene.as_mut() else {
        return;
    };
    if let Some(lights) = upload.lights {
        buffers.lights_upload = Some(lights);
    }
    if upload.resky {
        buffers.sky_done = false;
    }
    // Only an upload that carries a change says which generation it is: an
    // empty one is `Default`, generation 0, and taking that would roll a bake
    // still waiting on its pipeline back to a generation long gone.
    if !upload.dirty.is_empty() {
        bake.generation = upload.generation;
    }
    for chunk in upload.dirty {
        bake.pending[chunk as usize] = true;
        bake.pending_any = true;
    }
}

impl SceneBuffers {
    /// Buffers are made on first use, on the render thread; until then the
    /// upload is held.
    fn pending(upload: StaticUpload) -> Self {
        Self {
            image: upload.image.clone(),
            baked: upload.baked.clone(),
            size: upload.size,
            fixed: None,
            lights: None,
            sky_done: false,
            upload: Some(upload),
            lights_upload: None,
        }
    }
}

#[derive(Resource)]
struct LightPipelines {
    bake_layout: BindGroupLayoutDescriptor,
    bake: CachedComputePipelineId,
    sky_layout: BindGroupLayoutDescriptor,
    sky_init: CachedComputePipelineId,
    sky_step: CachedComputePipelineId,
}

fn init_pipelines(mut commands: Commands, assets: Res<AssetServer>, cache: Res<PipelineCache>) {
    let bake_layout = BindGroupLayoutDescriptor::new(
        "lightmap bake",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                uniform_buffer_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                texture_storage_2d(TextureFormat::Rgba16Float, StorageTextureAccess::WriteOnly),
            ),
        ),
    );
    let sky_layout = BindGroupLayoutDescriptor::new(
        "lightmap sky",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                uniform_buffer_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                storage_buffer_sized(false, None),
            ),
        ),
    );
    let compute = |label: &'static str, layout: &BindGroupLayoutDescriptor, shader: &'static str, entry: &'static str| {
        cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(label.into()),
            layout: vec![layout.clone()],
            shader: assets.load(shader),
            entry_point: Some(entry.into()),
            ..default()
        })
    };
    let bake = compute("lightmap bake", &bake_layout, "shaders/lighting.wgsl", "bake");
    let sky_init = compute("lightmap sky init", &sky_layout, "shaders/sky.wgsl", "sky_init");
    let sky_step = compute("lightmap sky step", &sky_layout, "shaders/sky.wgsl", "sky_step");
    commands.insert_resource(LightPipelines { bake_layout, bake, sky_layout, sky_init, sky_step });
}

/// A storage buffer may not be empty; a scene with no lights still binds one.
fn bytes_of_u32(data: &[u32]) -> Vec<u8> {
    let mut out: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
    if out.is_empty() {
        out.resize(16, 0);
    }
    out
}

fn bytes_of_f32(data: &[f32]) -> Vec<u8> {
    let mut out: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
    if out.is_empty() {
        out.resize(32, 0);
    }
    out
}

fn bake(
    pipelines: Option<Res<LightPipelines>>,
    cache: Res<PipelineCache>,
    mut bake: ResMut<LightBake>,
    images: Res<RenderAssets<GpuImage>>,
    mut ctx: RenderContext,
) {
    if !bake.pending_any {
        return;
    }
    let Some(pipelines) = pipelines else { return };
    let (Some(bake_pipeline), Some(sky_init), Some(sky_step)) = (
        cache.get_compute_pipeline(pipelines.bake),
        cache.get_compute_pipeline(pipelines.sky_init),
        cache.get_compute_pipeline(pipelines.sky_step),
    ) else {
        return;
    };
    let bake = &mut *bake;
    let Some(scene) = bake.scene.as_mut() else { return };
    let Some(image) = images.get(&scene.image) else { return };

    let device = ctx.render_device().clone();
    let make = |label: &str, usage: BufferUsages, contents: &[u8]| {
        device.create_buffer_with_data(&BufferInitDescriptor { label: Some(label), contents, usage })
    };
    if let Some(upload) = scene.upload.take() {
        let params = [upload.size.x, upload.size.y, upload.chunks_x, CHUNK];
        // The step costs travel as the CPU's own bits: see `sky.wgsl`.
        let sky_params: Vec<u8> = [upload.size.x, upload.size.y, upload.map_width, SUBTILES as u32]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .chain([SKY_STEP, SKY_STEP_DIAGONAL, 0.0, 0.0].iter().flat_map(|v| v.to_le_bytes()))
            .collect();
        let texels = (upload.size.x * upload.size.y) as usize;
        let sky = || make("lightmap sky", BufferUsages::STORAGE, &bytes_of_f32(&vec![0.0; texels]));
        scene.fixed = Some(Fixed {
            params: make("lightmap params", BufferUsages::UNIFORM, &bytes_of_u32(&params)),
            sky_params: make("lightmap sky params", BufferUsages::UNIFORM, &sky_params),
            occlusion: make("lightmap occlusion", BufferUsages::STORAGE, &bytes_of_u32(&upload.occlusion)),
            ceiling: make("lightmap ceiling", BufferUsages::STORAGE, &bytes_of_u32(&upload.ceiling)),
            chunk_offsets: make(
                "lightmap chunk offsets",
                BufferUsages::STORAGE,
                &bytes_of_u32(&upload.chunk_offsets),
            ),
            chunk_lights: make(
                "lightmap chunk lights",
                BufferUsages::STORAGE,
                &bytes_of_u32(&upload.chunk_lights),
            ),
            sky: [sky(), sky()],
        });
        scene.sky_done = false;
    }
    if let Some(lights) = scene.lights_upload.take() {
        scene.lights = Some(make("lightmap lights", BufferUsages::STORAGE, &bytes_of_f32(&lights)));
    }
    let Some(fixed) = &scene.fixed else { return };
    let lights = scene
        .lights
        .get_or_insert_with(|| make("lightmap lights", BufferUsages::STORAGE, &bytes_of_f32(&[])));

    let dirty: Vec<u32> = bake
        .pending
        .iter()
        .enumerate()
        .filter_map(|(i, &d)| d.then_some(i as u32))
        .collect();
    let dirty_buffer = make("lightmap dirty chunks", BufferUsages::STORAGE, &bytes_of_u32(&dirty));

    let sky_layout = cache.get_bind_group_layout(&pipelines.sky_layout);
    // `into[k]` writes `sky[k]`, reading the other one.
    let into = [(1, 0), (0, 1)].map(|(from, to): (usize, usize)| {
        device.create_bind_group(
            "lightmap sky",
            &sky_layout,
            &BindGroupEntries::sequential((
                fixed.sky_params.as_entire_binding(),
                fixed.occlusion.as_entire_binding(),
                fixed.ceiling.as_entire_binding(),
                fixed.sky[from].as_entire_binding(),
                fixed.sky[to].as_entire_binding(),
            )),
        )
    });
    let bind_group = device.create_bind_group(
        "lightmap bake",
        &cache.get_bind_group_layout(&pipelines.bake_layout),
        &BindGroupEntries::sequential((
            fixed.params.as_entire_binding(),
            fixed.occlusion.as_entire_binding(),
            lights.as_entire_binding(),
            fixed.chunk_offsets.as_entire_binding(),
            fixed.chunk_lights.as_entire_binding(),
            dirty_buffer.as_entire_binding(),
            fixed.sky[0].as_entire_binding(),
            &image.texture_view,
        )),
    );

    let per_chunk = CHUNK / WORKGROUP;
    let whole = (scene.size.x.div_ceil(WORKGROUP), scene.size.y.div_ceil(WORKGROUP));
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(ctx.command_encoder(), "lightmap bake");
    {
        let mut pass = ctx
            .command_encoder()
            .begin_compute_pass(&ComputePassDescriptor { label: Some("lightmap bake"), timestamp_writes: None });
        if !scene.sky_done {
            // Each dispatch sees the last one's writes: WebGPU puts a usage
            // boundary between dispatches, which is what a ping-pong needs.
            pass.set_pipeline(sky_init);
            pass.set_bind_group(0, &into[0], &[]);
            pass.dispatch_workgroups(whole.0, whole.1, 1);
            pass.set_pipeline(sky_step);
            // An even number of steps, so the field ends where it started:
            // in `sky[0]`, which the bake reads.
            const { assert!(SKY_ITERATIONS % 2 == 0) };
            for step in 0..SKY_ITERATIONS {
                pass.set_bind_group(0, &into[(step as usize + 1) % 2], &[]);
                pass.dispatch_workgroups(whole.0, whole.1, 1);
            }
        }
        pass.set_pipeline(bake_pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        // One z layer per dirty chunk. The z dimension is capped at 65535,
        // which is 4096x4096 cells of chunks; a bigger map would split this.
        pass.dispatch_workgroups(per_chunk, per_chunk, dirty.len() as u32);
    }
    span.end(ctx.command_encoder());
    trace!("lighting: baked {} chunks", dirty.len());

    scene.sky_done = true;
    bake.pending.iter_mut().for_each(|d| *d = false);
    bake.pending_any = false;
    scene.baked.store(bake.generation, Ordering::Release);
}
