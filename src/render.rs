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

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::window::{PrimaryWindow, WindowResized};

/// Every world pixel becomes a PIXEL_SCALE x PIXEL_SCALE block on screen.
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
        app.add_systems(Startup, setup_pipeline)
            .add_systems(Update, (resize_canvas, snap_camera_to_pixels));
    }
}

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
/// always covers the whole window even when the size is not a multiple of
/// `PIXEL_SCALE`.
fn canvas_size_for(window: &Window) -> UVec2 {
    let physical = UVec2::new(window.physical_width(), window.physical_height());
    (physical + PIXEL_SCALE - 1) / PIXEL_SCALE
}

fn setup_pipeline(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let size = windows
        .single()
        .map(canvas_size_for)
        .unwrap_or(UVec2::new(320, 180));
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

    // Draws the canvas to the window, PIXEL_SCALE times bigger.
    commands.spawn((
        Name::new("canvas quad"),
        Sprite {
            image: image.clone(),
            custom_size: Some(size.as_vec2()),
            ..default()
        },
        Transform::from_scale(Vec3::splat(PIXEL_SCALE as f32)),
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
}

/// Rebuild the canvas at the new resolution whenever the window changes size,
/// so the upscale factor stays exactly `PIXEL_SCALE` instead of stretching.
fn resize_canvas(
    mut resized: MessageReader<WindowResized>,
    mut images: ResMut<Assets<Image>>,
    mut canvas: ResMut<PixelCanvas>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut quads: Query<&mut Sprite, With<CanvasQuad>>,
    mut targets: Query<&mut RenderTarget, With<WorldCamera>>,
) {
    if resized.read().count() == 0 {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let size = canvas_size_for(window);
    if size == canvas.size || size.x == 0 || size.y == 0 {
        return;
    }

    let image = images.add(create_canvas_image(size));
    for mut sprite in &mut quads {
        sprite.image = image.clone();
        sprite.custom_size = Some(size.as_vec2());
    }
    for mut target in &mut targets {
        *target = RenderTarget::Image(image.clone().into());
    }
    images.remove(&canvas.image);
    canvas.image = image;
    canvas.size = size;
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
/// `camera` is the world camera's (already snapped) translation.
pub fn cursor_world_pos(window: &Window, camera: Vec2) -> Option<Vec2> {
    let window_size = Vec2::new(window.width(), window.height());
    Some(window_to_world(window.cursor_position()?, window_size, camera))
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
/// physical pixels and the divisor really is `PIXEL_SCALE`.
fn window_to_world(cursor: Vec2, window: Vec2, camera: Vec2) -> Vec2 {
    let offset = (cursor - window * 0.5) / PIXEL_SCALE as f32;
    camera + Vec2::new(offset.x, -offset.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Vec2 = Vec2::new(1280.0, 720.0);

    #[test]
    fn centre_of_the_window_is_the_camera() {
        let camera = Vec2::new(37.0, -12.0);
        assert_eq!(window_to_world(WINDOW * 0.5, WINDOW, camera), camera);
    }

    #[test]
    fn one_canvas_pixel_is_pixel_scale_window_pixels() {
        let scale = PIXEL_SCALE as f32;
        let cursor = WINDOW * 0.5 + Vec2::new(scale, 0.0);
        assert_eq!(window_to_world(cursor, WINDOW, Vec2::ZERO), Vec2::new(1.0, 0.0));
    }

    #[test]
    fn the_y_axis_is_flipped() {
        // Cursor below the centre of the window is below the camera in world
        // space, which is *negative* y.
        let cursor = WINDOW * 0.5 + Vec2::new(0.0, PIXEL_SCALE as f32);
        assert_eq!(window_to_world(cursor, WINDOW, Vec2::ZERO), Vec2::new(0.0, -1.0));
    }

    #[test]
    fn the_corners_span_the_canvas() {
        // A 1280x720 window is a 320x180 canvas, so the top-left corner is
        // half of that up and to the left of a camera at the origin.
        let top_left = window_to_world(Vec2::ZERO, WINDOW, Vec2::ZERO);
        assert_eq!(top_left, Vec2::new(-160.0, 90.0));
        let bottom_right = window_to_world(WINDOW, WINDOW, Vec2::ZERO);
        assert_eq!(bottom_right, Vec2::new(160.0, -90.0));
    }
}
