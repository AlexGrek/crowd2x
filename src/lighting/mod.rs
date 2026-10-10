//! Lighting: an RGB lightmap at five subtiles a cell, computed on the GPU
//! by a compute shader, and only where something changed.
//!
//! Three parts, each a step further from the simulation:
//!
//! * [`scene`] — plain Rust: what the lightmap is computed from (occlusion
//!   bits, the lights, which chunk each light reaches) and which chunks are
//!   dirty. Also the CPU reference the shader is checked against.
//! * this module — the main-world half: builds a [`LightScene`] from the
//!   simulated map when the game screen opens, switches lights the
//!   simulation turns on and off, and hands each frame's dirty chunks to the
//!   render world.
//! * [`gpu`] — the render-world half: the buffers and the compute dispatch,
//!   in `RenderGraph`'s `Render` set, ahead of every camera.
//!
//! Nothing is recomputed per frame. A frame with no dirty chunk dispatches
//! nothing and uploads nothing.

pub mod gpu;
pub mod scene;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};

use crate::game::actors::Sim;
use crate::map::{Point, PIXELS_PER_CELL};
use crate::render::CanvasLight;
use crate::state::AppState;

pub use scene::LightScene;

/// Set to check the GPU's lightmap against the CPU reference once it is
/// baked, and log the result.
const ENV_CHECK: &str = "CROWD2X_LIGHT_CHECK";

/// Set to re-bake the whole lightmap every frame, which is what a frame
/// timing needs to see the cost of one full bake.
const ENV_STRESS: &str = "CROWD2X_LIGHT_STRESS";

/// A read-back of the lightmap, compared with the CPU reference: asked for
/// by generation, and answered only once that generation has been baked.
/// The QA harness's `expect_lighting` and `CROWD2X_LIGHT_CHECK` both use it.
#[derive(Resource, Default)]
pub struct LightProbe {
    asked: Option<u64>,
    /// The read-back entity while one is outstanding.
    in_flight: Option<Entity>,
    result: Option<(u64, CheckReport)>,
}

impl LightProbe {
    pub fn ask(&mut self, generation: u64) {
        if self.asked != Some(generation) {
            self.asked = Some(generation);
        }
    }

    pub fn result_for(&self, generation: u64) -> Option<&CheckReport> {
        self.result.as_ref().filter(|(g, _)| *g == generation).map(|(_, r)| r)
    }
}

pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LightUpload>()
            .init_resource::<LightProbe>()
            .add_systems(OnExit(AppState::Game), drop_lighting)
            .add_systems(
                Update,
                (build_lighting, follow_power, switch_lights_in_use, publish_dirty_chunks, light_the_canvas)
                    .chain()
                    .after(crate::game::CrowdSystems)
                    .run_if(in_state(AppState::Game).and_then(resource_exists::<Sim>)),
            )
            .add_systems(
                Update,
                (
                    ask_from_env.run_if(|| std::env::var_os(ENV_CHECK).is_some()),
                    read_back_when_baked,
                )
                    .chain()
                    .after(publish_dirty_chunks)
                    .run_if(resource_exists::<Lighting>),
            );
        if std::env::var_os(ENV_STRESS).is_some() {
            // GPU timestamps for the bake, logged every second.
            app.add_plugins((
                bevy::render::diagnostic::RenderDiagnosticsPlugin,
                bevy::diagnostic::LogDiagnosticsPlugin::filtered(
                    ["render/lightmap bake/elapsed_gpu", "render/lightmap bake/elapsed_cpu"]
                        .map(bevy::diagnostic::DiagnosticPath::new)
                        .into_iter()
                        .collect(),
                ),
            ));
        }
        gpu::build(app);
    }
}

/// The lighting of the world on screen. Exists only on the game screen.
#[derive(Resource)]
pub struct Lighting {
    pub scene: LightScene,
    /// The lightmap, `scene.width` x `scene.height`, written only by the
    /// compute shader.
    pub image: Handle<Image>,
    /// Which [`LightScene::generation`] the GPU has finished baking. Written
    /// by the render world after a dispatch.
    pub baked: Arc<AtomicU64>,
    /// The switchable lights (a computer's screen), with the cell of the
    /// prop each came from: what is asked about every frame.
    switchable: Vec<(usize, Point)>,
    /// Lights currently switched on because somebody is using their prop.
    in_use: Vec<usize>,
    /// The [`GameState::supply_generation`](crate::sim::GameState::supply_generation)
    /// the lights were last switched for.
    supply_generation: u64,
}

impl Lighting {
    /// Whether the lightmap on the GPU matches the scene.
    pub fn is_baked(&self) -> bool {
        self.baked.load(Ordering::Acquire) == self.scene.generation()
    }
}

/// What the main world has to say to the render world this frame: taken
/// whole by the extract system, so it is said exactly once.
#[derive(Resource, Default)]
pub struct LightUpload {
    /// A new scene: everything static, and the image to bake into.
    pub scene: Option<gpu::StaticUpload>,
    /// The lights, whenever any changed (switching included).
    pub lights: Option<Vec<f32>>,
    pub dirty: Vec<u32>,
    pub generation: u64,
    /// Set when the game screen closed: drop everything.
    pub clear: bool,
    /// Propagate the sky again as well as baking: what a stress run times,
    /// since in play the sky is propagated once per scene.
    pub resky: bool,
}

fn build_lighting(
    mut commands: Commands,
    sim: Res<Sim>,
    lighting: Option<Res<Lighting>>,
    mut images: ResMut<Assets<Image>>,
    mut upload: ResMut<LightUpload>,
) {
    if lighting.is_some() {
        return;
    }
    let scene = LightScene::from_map(&sim.0.map, sim.0.supply());
    let mut image = Image::new_uninit(
        Extent3d { width: scene.width, height: scene.height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage =
        TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_SRC;
    let image = images.add(image);
    let baked = Arc::new(AtomicU64::new(u64::MAX));

    let switchable = scene
        .lights
        .iter()
        .enumerate()
        .filter(|(_, light)| light.switchable)
        .map(|(id, light)| (id, light.cell))
        .collect();

    upload.scene = Some(gpu::StaticUpload {
        image: image.clone(),
        baked: baked.clone(),
        size: UVec2::new(scene.width, scene.height),
        chunks_x: scene.chunks().0,
        map_width: scene.map_width,
        occlusion: scene.occlusion.clone(),
        ceiling: scene.ceiling.clone(),
        chunk_offsets: scene.chunk_offsets.clone(),
        chunk_lights: scene.chunk_lights.clone(),
    });
    upload.clear = false;
    info!(
        "lighting: {}x{} subtiles, {} lights, {} chunks",
        scene.width,
        scene.height,
        scene.lights.len(),
        scene.chunk_count()
    );
    let supply_generation = sim.0.supply_generation();
    commands.insert_resource(Lighting { scene, image, baked, switchable, in_use: Vec::new(), supply_generation });
}

/// A prop that lights up while it is used — a computer's screen — lights
/// the room too.
///
/// Asked of the simulation about each such prop, **not** of the crowd on
/// screen the way `game::props` asks: a screen's light reaches cells past
/// its own, so one in use just off the canvas lights part of it, and the
/// lightmap must not depend on where the camera is. Whoever is using a prop
/// stands in its cell or one beside it, so that is five occupancy lookups a
/// prop — cheap, and independent of the size of the crowd.
fn switch_lights_in_use(sim: Res<Sim>, lighting: Option<ResMut<Lighting>>, mut wanted: Local<Vec<usize>>) {
    let Some(mut lighting) = lighting else { return };
    let lighting = &mut *lighting;
    let world = &sim.0;
    wanted.clear();
    for &(id, cell) in &lighting.switchable {
        let used = std::iter::once(cell)
            .chain(cell.cardinal_neighbours())
            .filter_map(|at| world.occupancy().occupant(at))
            .filter_map(|uid| world.entities().get(uid))
            .any(|entity| entity.interacting_with() == Some(cell));
        if used {
            wanted.push(id);
        }
    }
    if *wanted == lighting.in_use {
        return;
    }
    for &id in &lighting.in_use {
        if wanted.binary_search(&id).is_err() {
            lighting.scene.set_enabled(id, false);
        }
    }
    for &id in wanted.iter() {
        lighting.scene.set_enabled(id, true);
    }
    lighting.in_use.clone_from(&wanted);
}

/// A distribution box switched: lamps on it go dark or light up again.
/// Nothing at all on a frame the power did not change, which is every
/// frame but the one a player clicked on.
fn follow_power(sim: Res<Sim>, lighting: Option<ResMut<Lighting>>) {
    let Some(mut lighting) = lighting else { return };
    let generation = sim.0.supply_generation();
    if generation == lighting.supply_generation {
        return;
    }
    lighting.supply_generation = generation;
    let changed = lighting.scene.apply_supply(sim.0.supply());
    info!("lighting: power changed, {changed} light(s) switched");
}

fn publish_dirty_chunks(lighting: Option<ResMut<Lighting>>, mut upload: ResMut<LightUpload>) {
    let Some(mut lighting) = lighting else { return };
    if std::env::var_os(ENV_STRESS).is_some() {
        lighting.scene.mark_all_dirty();
        upload.resky = true;
    }
    let dirty = lighting.scene.take_dirty();
    if dirty.is_empty() {
        return;
    }
    upload.lights = Some(lighting.scene.lights.iter().flat_map(|l| l.to_gpu()).collect());
    upload.dirty.extend(dirty);
    upload.generation = lighting.scene.generation();
}

/// Tell the canvas what to light it with: the lightmap, and the ambient
/// light of the world's time of day.
fn light_the_canvas(sim: Res<Sim>, lighting: Option<Res<Lighting>>, mut canvas: ResMut<CanvasLight>) {
    let Some(lighting) = lighting else { return };
    let clock = sim.0.clock();
    let hours = clock.hour() as f32 + clock.minute() as f32 / 60.0;
    let ambient = scene::ambient(hours);
    let wanted = CanvasLight {
        lightmap: Some(lighting.image.clone()),
        lightmap_size: UVec2::new(lighting.scene.width, lighting.scene.height),
        subtiles_per_unit: scene::SUBTILES as f32 / PIXELS_PER_CELL as f32,
        ambient: Vec3::from(ambient),
        indoor: Vec3::from(scene::indoor(ambient)),
    };
    // Only on a change: `CanvasLight` is change-detected downstream, and the
    // ambient moves once a world minute, not every frame.
    if *canvas != wanted {
        *canvas = wanted;
    }
}

fn drop_lighting(
    mut commands: Commands,
    mut upload: ResMut<LightUpload>,
    mut probe: ResMut<LightProbe>,
    mut canvas: ResMut<CanvasLight>,
) {
    commands.remove_resource::<Lighting>();
    *canvas = CanvasLight::default();
    if let Some(readback) = probe.in_flight {
        commands.entity(readback).try_despawn();
    }
    *probe = LightProbe::default();
    *upload = LightUpload { clear: true, ..default() };
}

/// Ask once, but follow the scene: a light switched before the first bake
/// moves the generation on, and a question about the old one would never be
/// answered.
fn ask_from_env(lighting: Res<Lighting>, mut probe: ResMut<LightProbe>, mut answered: Local<bool>) {
    if *answered {
        return;
    }
    let generation = lighting.scene.generation();
    if probe.result_for(generation).is_some() {
        *answered = true;
    } else {
        probe.ask(generation);
    }
}

/// Read the lightmap back once the generation asked about is baked, and
/// compare every texel with [`LightScene::evaluate`].
fn read_back_when_baked(mut commands: Commands, lighting: Res<Lighting>, mut probe: ResMut<LightProbe>) {
    let Some(generation) = probe.asked else { return };
    if probe.in_flight.is_some() || !lighting.is_baked() || lighting.scene.generation() != generation {
        return;
    }
    let readback = commands
        .spawn(Readback::texture(lighting.image.clone()))
        .observe(
        move |event: On<ReadbackComplete>,
              lighting: Option<Res<Lighting>>,
              mut probe: ResMut<LightProbe>,
              mut commands: Commands| {
            commands.entity(event.entity).try_despawn();
            if probe.in_flight != Some(event.entity) {
                // A read-back fires every frame until it is despawned, and
                // one from a screen since left is not this one.
                return;
            }
            probe.in_flight = None;
            probe.asked = None;
            let Some(lighting) = lighting else { return };
            let report = compare(&lighting.scene, &event.data);
            info!("lighting check (generation {generation}): {report}");
            probe.result = Some((generation, report));
        },
    )
        .id();
    probe.in_flight = Some(readback);
}

/// Every texel of a read-back `rgba16float` lightmap against the reference.
pub fn compare(scene: &LightScene, data: &[u8]) -> CheckReport {
    let row = (scene.width as usize * 8).next_multiple_of(256);
    let mut report = CheckReport::default();
    if data.len() < row * scene.height as usize {
        // Not this scene's lightmap: every texel counts as wrong.
        report.texels = scene.width * scene.height;
        report.wrong = report.texels * 4;
        return report;
    }
    let sky = scene.sky_field();
    for y in 0..scene.height {
        for x in 0..scene.width {
            let at = y as usize * row + x as usize * 8;
            let want = scene.evaluate(x, y, &sky);
            // RGB is the lights, alpha the sky factor: both baked, both checked.
            for c in 0..4 {
                let got = f16_to_f32(u16::from_le_bytes([data[at + 2 * c], data[at + 2 * c + 1]]));
                // f16 keeps 11 bits of mantissa: compare relative to the value.
                let err = (got - want[c]).abs() / want[c].abs().max(1.0);
                if err > report.max_error {
                    report.max_error = err;
                    report.worst = (x, y);
                }
                if err > 0.01 {
                    report.wrong += 1;
                }
                if c < 3 && got > 0.0 {
                    report.lit += 1;
                }
                if c == 3 && got > 0.0 && got < 1.0 {
                    report.shaded += 1;
                }
            }
        }
    }
    report.texels = scene.width * scene.height;
    report
}

#[derive(Default, Debug)]
pub struct CheckReport {
    pub texels: u32,
    pub lit: u32,
    /// Texels under a ceiling the sky still reaches: the sun spilling in.
    pub shaded: u32,
    pub wrong: u32,
    pub max_error: f32,
    pub worst: (u32, u32),
}

impl std::fmt::Display for CheckReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} texels, {} lit channels, {} in the half-light of a ceiling, {} channels off by more than 1%, worst {:.4} at {:?}",
            self.texels, self.lit, self.shaded, self.wrong, self.max_error, self.worst
        )
    }
}

pub fn f16_to_f32(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = ((bits >> 10) & 0x1f) as i32;
    let man = (bits & 0x3ff) as f32;
    match exp {
        0 => sign * man * 2f32.powi(-24),
        31 => if man == 0.0 { sign * f32::INFINITY } else { f32::NAN },
        _ => sign * (1.0 + man / 1024.0) * 2f32.powi(exp - 15),
    }
}
