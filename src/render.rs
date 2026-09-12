//! Pixel-perfect rendering pipeline.
//!
//! Everything in the game world is drawn by an in-world camera into an
//! off-screen [`Image`] whose size is exactly `window / PIXEL_SCALE`. A second
//! camera then draws that image to the window as a single sprite scaled by
//! `PIXEL_SCALE` with nearest-neighbour filtering.
//!
//! Doing it this way (rather than just zooming the in-world camera) guarantees
//! that every source texel becomes an exact NxN block of screen pixels: sprites
//! can never land on a half-texel, so there is no shimmering when the camera
//! moves and no uneven pixel sizes.
//!
//! **Zooming is changing N.** [`PixelZoom`] is that factor, and it is a whole
//! number for the same reason: a zoom of 2.5 would give neighbouring texels
//! different widths and break the grid the pipeline exists to protect. Zooming
//! in therefore makes the canvas *smaller* — fewer, bigger pixels — so the
//! canvas is rebuilt at the new resolution exactly as it is on a resize. Only
//! the canvas's *resolution* jumps, though: the quad blitting it to the
//! window eases its scale up to the new factor over a few frames
//! ([`ZoomQuadScale`]) rather than snapping there, so a zoom step still reads
//! as one whole step but does not feel like a cut.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::window::{PrimaryWindow, WindowResized};

/// Every world pixel becomes a PIXEL_SCALE x PIXEL_SCALE block on screen,
/// unless something has zoomed — see [`PixelZoom`], which starts here.
///
/// It is also what the interface is measured in (`UiScale`), and that does
/// *not* zoom: the HUD stays the same size on screen however far in the world
/// is zoomed, so the two uses are separate on purpose.
pub const PIXEL_SCALE: u32 = 4;

/// Layer for everything that is drawn *inside* the low-res canvas.
pub const WORLD_LAYER: RenderLayers = RenderLayers::layer(0);
/// Layer holding only the canvas quad itself.
const UPSCALE_LAYER: RenderLayers = RenderLayers::layer(1);

/// Colour the low-res canvas is cleared to each frame.
const CANVAS_CLEAR: Color = Color::srgb(0.10, 0.11, 0.14);

pub struct PixelRenderPlugin;

impl Plugin for PixelRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PixelZoom>()
            .init_resource::<CameraTarget>()
            .add_systems(Startup, setup_pipeline)
            // Chained: a camera moved this frame should be snapped this frame,
            // not photographed one frame off the pixel grid first.
            .add_systems(
                Update,
                (
                    resize_canvas,
                    ease_zoom_quad,
                    apply_camera_target,
                    snap_camera_to_pixels,
                )
                    .chain(),
            );
    }
}

/// Screen pixels per canvas pixel: the zoom.
///
/// Whole numbers only, which is the whole reason zooming lives here rather
/// than on the camera's scale. Changing it rebuilds the canvas at
/// `window / zoom`, so zooming in shows less of the world at a larger size and
/// every sprite stays on exact pixel blocks at either end.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelZoom(u32);

impl Default for PixelZoom {
    fn default() -> Self {
        Self(PIXEL_SCALE)
    }
}

impl PixelZoom {
    /// One screen pixel per canvas pixel: the whole map at its smallest, and
    /// the point where the art stops being legible.
    pub const MIN: u32 = 1;
    /// Twice the default. Past this a 48px cell fills a third of the window.
    pub const MAX: u32 = 8;

    /// A zoom chosen from outside — the debug harness's `CROWD2X_ZOOM`.
    /// Clamped rather than refused: an out-of-range number is a typo, and the
    /// nearest zoom that exists is a more useful answer than a panic.
    pub fn new(zoom: u32) -> Self {
        Self(zoom.clamp(Self::MIN, Self::MAX))
    }

    pub fn get(self) -> u32 {
        self.0
    }

    pub fn factor(self) -> f32 {
        self.0 as f32
    }

    /// Zoom by whole steps, clamped. Answers whether it actually moved, so a
    /// caller can leave the HUD alone when it did not.
    pub fn step(&mut self, steps: i32) -> bool {
        let wanted =
            (self.0 as i32 + steps).clamp(Self::MIN as i32, Self::MAX as i32) as u32;
        if wanted == self.0 {
            return false;
        }
        self.0 = wanted;
        true
    }

    pub fn reset(&mut self) {
        self.0 = PIXEL_SCALE;
    }
}

/// Where the camera should be put, once there is a camera to put it there.
///
/// Asking and applying are two steps rather than one because `OnEnter` for the
/// *initial* state runs before every `Startup` system: a screen booted into
/// directly reaches its own setup before [`setup_pipeline`] has spawned the
/// camera. Writing the wish here means that boot still lands where the screen
/// wanted instead of silently looking at the corner of the map.
#[derive(Resource, Default)]
pub struct CameraTarget(pub Option<Vec2>);

/// Handle of the off-screen canvas plus the resolution it was created at.
#[derive(Resource)]
pub struct PixelCanvas {
    pub image: Handle<Image>,
    pub size: UVec2,
}

/// Camera that renders the world into the canvas.
#[derive(Component)]
pub struct WorldCamera;

/// Camera that blits the canvas to the window.
#[derive(Component)]
struct UpscaleCamera;

/// The quad displaying the canvas.
#[derive(Component)]
struct CanvasQuad;

/// Sub-pixel camera position. The camera's own `Transform` is always snapped to
/// whole pixels; this keeps smooth movement possible without breaking that.
///
/// Anything that moves the camera writes here and never to the `Transform` —
/// see `editor::pan_camera`.
#[derive(Component, Default)]
pub struct CameraPan(pub Vec2);

/// The scale the canvas quad is actually drawn at, easing toward
/// [`PixelZoom`]'s factor rather than jumping to it.
///
/// The canvas itself still snaps to the new resolution the instant the zoom
/// changes — only a whole-number canvas keeps every texel the same size, and
/// that is still true at every point during the ease — so this is the *blit*
/// catching up to a step that has already happened underneath it, not a
/// second, fractional zoom. It lands exactly on the target (see
/// [`ease_toward`]) rather than trailing forever, so a screenshot taken once
/// the ease is done is exactly as pixel-perfect as before this existed.
#[derive(Resource)]
struct ZoomQuadScale(f32);

/// How quickly [`ZoomQuadScale`] closes the distance to the target zoom, in
/// closed-fraction-per-second: at this rate a step is visually settled well
/// under 300ms, which is brisk enough not to lag behind repeated key presses.
const ZOOM_EASE_RATE: f32 = 14.0;

/// Exponential ease toward `target`, snapping once the gap is not worth
/// another frame of interpolation.
///
/// Framerate-independent (the rate is continuous, not "a fraction per
/// frame"), and it terminates: without the snap, an exponential ease only
/// ever approaches its target and a screenshot taken after the game has been
/// sitting idle would still find the quad a fraction of a pixel off the grid.
fn ease_toward(current: f32, target: f32, dt: f32) -> f32 {
    const SNAP_EPSILON: f32 = 0.01;
    let eased = current + (target - current) * (1.0 - (-ZOOM_EASE_RATE * dt).exp());
    if (target - eased).abs() < SNAP_EPSILON {
        target
    } else {
        eased
    }
}

/// Allocate an image usable as a render target.
fn create_canvas_image(size: UVec2) -> Image {
    let extent = Extent3d {
        width: size.x,
        height: size.y,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new_fill(
        extent,
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
        | TextureUsages::COPY_DST
        | TextureUsages::RENDER_ATTACHMENT;
    image
}

/// Canvas resolution for a given window size, rounded up so the upscaled quad
/// always covers the whole window even when the size is not a multiple of the
/// zoom.
fn canvas_size_for(window: &Window, zoom: u32) -> UVec2 {
    (window_size(window) + zoom - 1) / zoom
}

fn window_size(window: &Window) -> UVec2 {
    UVec2::new(window.physical_width(), window.physical_height())
}

/// Half a pixel of nudge for the canvas quad, per axis, or nothing.
///
/// The quad is centred on the window, so its edge sits `(window - quad) / 2`
/// from the window's own edge. When the canvas was rounded up by an odd number
/// of screen pixels that lands the edge on *half* a pixel, and from there every
/// source texel straddles two screen pixels: not a blur — nothing is filtered —
/// but rows of blocks a pixel wider than their neighbours, which is the exact
/// unevenness this pipeline exists to prevent.
///
/// It shows up at any zoom whose rounding is odd (a window 602 tall at zoom 3
/// is a 201-pixel canvas drawn 603 tall), so it cannot be fixed by choosing a
/// better canvas size — only by putting the quad back on the grid.
fn quad_offset(window: UVec2, canvas: UVec2, zoom: u32) -> Vec2 {
    let quad = canvas * zoom;
    let odd = |quad: u32, window: u32| if (quad.abs_diff(window)) % 2 == 1 { 0.5 } else { 0.0 };
    Vec2::new(odd(quad.x, window.x), odd(quad.y, window.y))
}

fn setup_pipeline(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    zoom: Res<PixelZoom>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let window = windows.single().ok();
    let size = window
        .map(|window| canvas_size_for(window, zoom.get()))
        .unwrap_or(UVec2::new(320, 180));
    let offset = window
        .map(|window| quad_offset(window_size(window), size, zoom.get()))
        .unwrap_or(Vec2::ZERO);
    let image = images.add(create_canvas_image(size));

    // Renders the world at 1 world unit == 1 canvas pixel.
    commands.spawn((
        Name::new("world camera"),
        Camera2d,
        Camera {
            order: -1,
            clear_color: ClearColorConfig::Custom(CANVAS_CLEAR),
            ..default()
        },
        RenderTarget::Image(image.clone().into()),
        Msaa::Off,
        WORLD_LAYER,
        WorldCamera,
        CameraPan::default(),
    ));

    // Draws the canvas to the window, `zoom` times bigger.
    commands.spawn((
        Name::new("canvas quad"),
        Sprite {
            image: image.clone(),
            custom_size: Some(size.as_vec2()),
            ..default()
        },
        Transform::from_translation(offset.extend(0.0)).with_scale(Vec3::splat(zoom.factor())),
        UPSCALE_LAYER,
        CanvasQuad,
    ));

    commands.spawn((
        Name::new("upscale camera"),
        Camera2d,
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        Msaa::Off,
        UPSCALE_LAYER,
        UpscaleCamera,
        // The only camera targeting the window, so bevy_ui would pick it
        // anyway; saying so keeps that true if another camera turns up.
        IsDefaultUiCamera,
    ));

    commands.insert_resource(PixelCanvas { image, size });
    // Starts at the initial zoom exactly, not `PIXEL_SCALE`: `CROWD2X_ZOOM`
    // picks a starting zoom in `Plugin::build`, before this runs, and easing
    // from the default would put the very first frame off the pixel grid.
    commands.insert_resource(ZoomQuadScale(zoom.factor()));
}

/// Rebuild the canvas at the new resolution whenever the window changes size
/// or the zoom changes, so the upscale factor stays exactly the zoom instead
/// of stretching.
///
/// One system for both because they are the same event as far as the canvas is
/// concerned: the resolution it needs is `window / zoom`, and either half of
/// that can move.
fn resize_canvas(
    mut resized: MessageReader<WindowResized>,
    zoom: Res<PixelZoom>,
    mut images: ResMut<Assets<Image>>,
    mut canvas: ResMut<PixelCanvas>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut quads: Query<(&mut Sprite, &mut Transform), With<CanvasQuad>>,
    mut targets: Query<&mut RenderTarget, With<WorldCamera>>,
) {
    if resized.read().count() == 0 && !zoom.is_changed() {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let size = canvas_size_for(window, zoom.get());
    if size == canvas.size || size.x == 0 || size.y == 0 {
        return;
    }

    let offset = quad_offset(window_size(window), size, zoom.get());
    let image = images.add(create_canvas_image(size));
    for (mut sprite, mut transform) in &mut quads {
        sprite.image = image.clone();
        sprite.custom_size = Some(size.as_vec2());
        transform.translation = offset.extend(0.0);
        // Scale is not set here: the canvas resolution jumps to the new zoom
        // immediately (a fractional one would break the pixel grid), but the
        // quad's on-screen scale eases up to it in `ease_zoom_quad` instead
        // of jumping too, which is the whole of the "smoothed" zoom.
    }
    for mut target in &mut targets {
        *target = RenderTarget::Image(image.clone().into());
    }
    images.remove(&canvas.image);
    canvas.image = image;
    canvas.size = size;
}

/// Ease the canvas quad's scale toward the current zoom instead of jumping to
/// it, so a zoom step is felt as a brisk animation rather than a snap — while
/// [`resize_canvas`] has already, in the same frame, rebuilt the canvas at the
/// new resolution, so what is inside the quad is never itself stretched.
fn ease_zoom_quad(
    time: Res<Time>,
    zoom: Res<PixelZoom>,
    mut scale: ResMut<ZoomQuadScale>,
    mut quads: Query<&mut Transform, With<CanvasQuad>>,
) {
    scale.0 = ease_toward(scale.0, zoom.factor(), time.delta_secs());
    for mut transform in &mut quads {
        transform.scale = Vec3::splat(scale.0);
    }
}

/// Put the camera where a screen asked for it, once there is one to put.
fn apply_camera_target(
    mut target: ResMut<CameraTarget>,
    mut cameras: Query<&mut CameraPan, With<WorldCamera>>,
) {
    let Some(wanted) = target.0 else {
        return;
    };
    let mut moved = false;
    for mut pan in &mut cameras {
        pan.0 = wanted;
        moved = true;
    }
    if moved {
        target.0 = None;
    }
}

/// Keep the rendered camera position on whole canvas pixels, whatever is
/// driving it. Without this a camera at a fractional offset samples every
/// sprite between texels and the whole grid shimmers.
fn snap_camera_to_pixels(mut cameras: Query<(&mut Transform, &CameraPan), With<WorldCamera>>) {
    for (mut transform, pan) in &mut cameras {
        transform.translation.x = pan.0.x.round();
        transform.translation.y = pan.0.y.round();
    }
}

/// World position under the mouse cursor, or `None` when the cursor is outside
/// the window.
///
/// `camera` is the world camera's (already snapped) translation, and `zoom`
/// the factor the canvas is currently drawn at.
pub fn cursor_world_pos(window: &Window, camera: Vec2, zoom: u32) -> Option<Vec2> {
    let window_size = Vec2::new(window.width(), window.height());
    Some(window_to_world(
        window.cursor_position()?,
        window_size,
        camera,
        zoom,
    ))
}

/// How much world the window shows, in world units, at this zoom.
///
/// Half of it, because every caller wants the distance from the middle: what
/// the camera can see either side of itself is what decides how far it may be
/// panned before the map runs out.
pub fn half_view(window: &Window, zoom: u32) -> Vec2 {
    Vec2::new(window.width(), window.height()) / (2.0 * zoom as f32)
}

/// The mapping itself, split out from the `Window` so it can be tested.
///
/// The canvas quad is centred on the window, so an offset from the window
/// centre is the same offset in canvas pixels once divided by `PIXEL_SCALE`.
/// That holds even when the canvas was rounded up past the window size: the
/// quad grows around its centre, which is still the centre of the window.
///
/// Window coordinates are in logical pixels running *down* the screen. The
/// window's scale factor is overridden to 1.0 in `main`, so logical pixels are
/// physical pixels and the divisor really is the zoom.
fn window_to_world(cursor: Vec2, window: Vec2, camera: Vec2, zoom: u32) -> Vec2 {
    let offset = (cursor - window * 0.5) / zoom as f32;
    camera + Vec2::new(offset.x, -offset.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Vec2 = Vec2::new(1280.0, 720.0);

    #[test]
    fn centre_of_the_window_is_the_camera() {
        let camera = Vec2::new(37.0, -12.0);
        assert_eq!(
            window_to_world(WINDOW * 0.5, WINDOW, camera, PIXEL_SCALE),
            camera
        );
    }

    #[test]
    fn one_canvas_pixel_is_pixel_scale_window_pixels() {
        let scale = PIXEL_SCALE as f32;
        let cursor = WINDOW * 0.5 + Vec2::new(scale, 0.0);
        assert_eq!(
            window_to_world(cursor, WINDOW, Vec2::ZERO, PIXEL_SCALE),
            Vec2::new(1.0, 0.0)
        );
    }

    #[test]
    fn the_y_axis_is_flipped() {
        // Cursor below the centre of the window is below the camera in world
        // space, which is *negative* y.
        let cursor = WINDOW * 0.5 + Vec2::new(0.0, PIXEL_SCALE as f32);
        assert_eq!(
            window_to_world(cursor, WINDOW, Vec2::ZERO, PIXEL_SCALE),
            Vec2::new(0.0, -1.0)
        );
    }

    #[test]
    fn the_corners_span_the_canvas() {
        // A 1280x720 window is a 320x180 canvas, so the top-left corner is
        // half of that up and to the left of a camera at the origin.
        let top_left = window_to_world(Vec2::ZERO, WINDOW, Vec2::ZERO, PIXEL_SCALE);
        assert_eq!(top_left, Vec2::new(-160.0, 90.0));
        let bottom_right = window_to_world(WINDOW, WINDOW, Vec2::ZERO, PIXEL_SCALE);
        assert_eq!(bottom_right, Vec2::new(160.0, -90.0));
    }

    /// The same screen position is a different world position once zoomed:
    /// a cursor mapped at the wrong factor paints somewhere other than where
    /// it is pointing, which is the bug this argument exists to prevent.
    #[test]
    fn zooming_in_puts_the_same_pixel_closer_to_the_camera() {
        let cursor = WINDOW * 0.5 + Vec2::new(80.0, 0.0);
        assert_eq!(
            window_to_world(cursor, WINDOW, Vec2::ZERO, 4),
            Vec2::new(20.0, 0.0)
        );
        assert_eq!(
            window_to_world(cursor, WINDOW, Vec2::ZERO, 8),
            Vec2::new(10.0, 0.0)
        );
        assert_eq!(
            window_to_world(cursor, WINDOW, Vec2::ZERO, 1),
            Vec2::new(80.0, 0.0)
        );
    }

    /// A canvas rounded up by an odd number of screen pixels puts the quad's
    /// edge on half a pixel; half a pixel of nudge puts it back.
    #[test]
    fn the_canvas_quad_lands_on_whole_pixels_at_every_zoom() {
        for zoom in PixelZoom::MIN..=PixelZoom::MAX {
            for window in [UVec2::new(1280, 720), UVec2::new(1002, 602), UVec2::new(999, 601)] {
                let canvas = (window + zoom - 1) / zoom;
                let offset = quad_offset(window, canvas, zoom);
                // Where the quad's left and bottom edges land, in window
                // pixels from the window's own edge.
                let edge = (window.as_vec2() - (canvas * zoom).as_vec2()) / 2.0 + offset;
                assert_eq!(edge.x.fract(), 0.0, "{zoom}x in {window}");
                assert_eq!(edge.y.fract(), 0.0, "{zoom}x in {window}");
            }
        }
    }

    #[test]
    fn a_canvas_covers_the_window_at_every_zoom() {
        // Rounded up, never down: a canvas a pixel short of the window leaves
        // an unpainted strip down the edge of the screen.
        for zoom in PixelZoom::MIN..=PixelZoom::MAX {
            let size = (UVec2::new(1002, 602) + zoom - 1) / zoom;
            assert!(size.x * zoom >= 1002 && size.y * zoom >= 602, "zoom {zoom}");
        }
    }

    #[test]
    fn the_zoom_stops_at_both_ends_and_says_when_it_did_not_move() {
        let mut zoom = PixelZoom::default();
        assert_eq!(zoom.get(), PIXEL_SCALE);
        assert!(zoom.step(1));
        assert_eq!(zoom.get(), PIXEL_SCALE + 1);

        assert!(zoom.step(100));
        assert_eq!(zoom.get(), PixelZoom::MAX);
        // Already at the end: nothing moved, so nothing needs redrawing.
        assert!(!zoom.step(1));

        assert!(zoom.step(-100));
        assert_eq!(zoom.get(), PixelZoom::MIN);
        assert!(!zoom.step(-1));

        zoom.reset();
        assert_eq!(zoom.get(), PIXEL_SCALE);
    }

    /// Repeated small steps reach the target and stop there, rather than
    /// creeping toward it forever — a screenshot taken any time after the game
    /// has been idle a moment must find the quad on an exact zoom.
    #[test]
    fn easing_toward_a_zoom_arrives_and_stops() {
        let mut scale = 4.0_f32;
        let target = 6.0_f32;
        for _ in 0..120 {
            let next = ease_toward(scale, target, 1.0 / 60.0);
            assert!(
                (next - target).abs() <= (scale - target).abs(),
                "must not overshoot or oscillate"
            );
            scale = next;
        }
        assert_eq!(scale, target);
    }

    /// A target already reached is a no-op, not a divide-by-zero or a tiny
    /// oscillation around it.
    #[test]
    fn easing_toward_the_current_value_does_nothing() {
        assert_eq!(ease_toward(5.0, 5.0, 1.0 / 60.0), 5.0);
    }
}
