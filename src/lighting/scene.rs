//! What the lightmap is computed *from*: plain Rust, no `bevy::` imports.
//!
//! The GPU does the arithmetic (`assets/shaders/lighting.wgsl`); this is
//! everything it is handed, and the bookkeeping that decides when it has to
//! be handed anything at all. Lighting is event-driven: nothing is computed
//! per frame, only the chunks a change touched.
//!
//! # Subtiles
//!
//! A map cell is [`SUBTILES`] x [`SUBTILES`] lighting cells, so a lightmap is
//! five times the map's size on each axis. Occlusion is kept at the same
//! resolution — one bit per subtile — which is what lets a wall's lit face be
//! one subtile deep rather than a whole cell, and what a wall thinner than a
//! cell will need when there is one.
//!
//! # Direct visibility
//!
//! A texel is lit by a light when the straight line between them crosses no
//! opaque subtile (an Amanatides-Woo walk over the subtile grid), attenuated
//! by `(1 - d²/r²)²`. The texel's *own* subtile may be opaque — that is a
//! wall's face, and it is lit — but nothing between it and the light may be.
//! Contributions are summed in `f32` and stored as `f16`, so overlapping
//! lights do not saturate before they are drawn.
//!
//! # The sky
//!
//! Under open sky a texel gets the sun — the ambient light of the time of
//! day — whole. Under a ceiling (`Map::has_ceiling`) it gets what spills in,
//! and spilling is **grid propagation**, not rays: the sky flows from subtile
//! to subtile in eight directions, losing [`SKY_STEP`] per step (a diagonal
//! step [`SKY_STEP_DIAGONAL`]), through doorways and round corners, never
//! through anything opaque and never diagonally between two opaque subtiles
//! that meet at a corner. It is a distance field, gone to nothing
//! [`SKY_ITERATIONS`] subtiles in. Rays were tried first and alias: sixteen of
//! them see a doorway two cells away between two rays, and a room comes out in
//! streaks where some hit and some miss.
//!
//! The **sky factor** stored in the lightmap's alpha is that field squared,
//! for a soft shoulder: 1 outside, ~0.7 a cell inside a doorway, ~0.25 three
//! cells in, nothing at the back of a deep room. It depends on the walls and
//! the ceiling alone, never on the time, so it is baked once per scene and
//! the sun can move all day without a re-bake — the canvas multiplies it by
//! the sun of the moment.
//!
//! On the GPU it is [`SKY_ITERATIONS`] ping-pong passes over the whole map
//! (Bellman-Ford: a value is the best path of at most that many steps, and no
//! longer path is worth anything). Subtraction and `max` are correctly
//! rounded in WGSL as in Rust, so [`LightScene::sky_field`] reproduces it to
//! the bit.
//!
//! # Chunks
//!
//! The lightmap is cut into [`CHUNK`]-subtile squares, and each chunk knows
//! which lights can reach it (a CSR index: [`LightScene::chunk_offsets`] into
//! [`LightScene::chunk_lights`]). A texel only ever looks at its own chunk's
//! list, so a thousand lights on a map cost each texel the handful that are
//! near it. A change to a light dirties the chunks under its radius and no
//! others, and only dirty chunks are dispatched.

use crate::map::utilities::Supply;
use crate::map::{Map, ObjectLayer, Point, TerrainId, PIXELS_PER_CELL};

/// Lighting cells per map cell, on each axis.
pub const SUBTILES: i32 = 5;

/// A chunk's side in subtiles: eight cells. A multiple of the shader's
/// workgroup side (8), so a chunk is a whole number of workgroups.
pub const CHUNK: u32 = 40;

/// How close two crossing times must be to count as one: the line passing
/// exactly through a subtile corner. Lights on prop centres sit on subtile
/// centres, so exact corners are common, and below this the CPU's and the
/// GPU's rounding would otherwise pick different sides of the corner. Far
/// below the smallest real gap between crossings within a light's radius.
pub const CORNER: f32 = 1e-5;

/// What one orthogonal step under a ceiling costs the sky: a thirtieth, so
/// it is gone [`SKY_ITERATIONS`] subtiles (six cells) from the nearest open
/// sky — about the depth of a room, so the sun lights the front of one and
/// leaves the back to its lamps.
pub const SKY_STEP: f32 = 1.0 / 30.0;

/// A diagonal step: √2 of an orthogonal one, so the field is round-ish
/// (octagonal) rather than a diamond.
pub const SKY_STEP_DIAGONAL: f32 = SKY_STEP * 1.414_213_6;

/// Propagation passes: enough for the sky to run out along the cheapest
/// path, so the field has converged and there is no edge where it stopped.
pub const SKY_ITERATIONS: u32 = 30;

/// The shader's workgroup side. Must match `@workgroup_size` in
/// `lighting.wgsl`.
pub const WORKGROUP: u32 = 8;

/// What a prop or a lamp gives off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Emission {
    /// Linear RGB, already multiplied by intensity.
    pub color: [f32; 3],
    /// How far it reaches, in **cells**.
    pub radius: f32,
    /// Whether it is lit from the start. A computer's screen is only on
    /// while somebody is at it, so it starts dark and is switched.
    pub always_on: bool,
}

/// What gives off light, by the name a map stores it under — a prop's (the
/// same key `map::PROPS` and the props palette use) or a lamp's (the lamps
/// palette's).
pub const EMITTERS: &[(&str, Emission)] = &[
    // A warm bulb over a room: lights a house's room from the middle.
    (
        "ceiling lamp",
        Emission { color: [0.95, 0.82, 0.6], radius: 4.5, always_on: true },
    ),
    // A cool strip light: an office's, set out in rows.
    (
        "tube lamp",
        Emission { color: [0.78, 0.86, 0.95], radius: 5.0, always_on: true },
    ),
    (
        "fire",
        Emission { color: [1.1, 0.62, 0.28], radius: 6.0, always_on: true },
    ),
    (
        "computer",
        Emission { color: [0.35, 0.6, 0.9], radius: 2.5, always_on: false },
    ),
];

pub fn emission(name: &str) -> Option<Emission> {
    EMITTERS.iter().find(|(n, _)| *n == name).map(|(_, e)| *e)
}

/// The light everywhere, before any lamp, at a time of day (hours since
/// midnight, fractional): white by day, so a day looks exactly as the art was
/// drawn; a warm dusk and dawn; a dark blue night, from 21:00 to 05:00, in
/// which a lamp is what lights a room.
pub fn ambient(hours: f32) -> [f32; 3] {
    const NIGHT: [f32; 3] = [0.12, 0.14, 0.26];
    const DAWN: [f32; 3] = [0.55, 0.48, 0.55];
    const DAY: [f32; 3] = [1.0, 1.0, 1.0];
    const DUSK: [f32; 3] = [0.75, 0.5, 0.38];
    const KEYS: [(f32, [f32; 3]); 8] = [
        (0.0, NIGHT),
        (5.0, NIGHT),
        (6.0, DAWN),
        (7.0, DAY),
        (19.0, DAY),
        (20.0, DUSK),
        (21.0, NIGHT),
        (24.0, NIGHT),
    ];
    let h = hours.rem_euclid(24.0);
    let i = KEYS.iter().rposition(|(at, _)| *at <= h).unwrap_or(0).min(KEYS.len() - 2);
    let (h0, a) = KEYS[i];
    let (h1, b) = KEYS[i + 1];
    let t = ((h - h0) / (h1 - h0)).clamp(0.0, 1.0);
    std::array::from_fn(|c| a[c] + (b[c] - a[c]) * t)
}

/// The light under a ceiling no sky reaches, before any lamp: a little of
/// whatever the sun is doing — light gets in round doors and through walls a
/// renderer does not model — and a floor under it, so a room with no lamp is
/// dark but legible rather than black.
pub fn indoor(ambient: [f32; 3]) -> [f32; 3] {
    ambient.map(|c| 0.04 + 0.12 * c)
}

/// One light as the shader reads it: 8 floats, std430-compatible —
/// `pos: vec2<f32>, radius: f32, enabled: f32, color: vec4<f32>`.
/// Position and radius are in subtiles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub pos: [f32; 2],
    pub radius: f32,
    pub enabled: bool,
    pub color: [f32; 3],
    /// Switched by the game (a computer in use) rather than always on.
    pub switchable: bool,
    /// Gives no light without power ([`crate::map::utilities::CONSUMERS`]):
    /// a lamp or a computer, not a fire. Such a light is in the scene
    /// whether it has power or not, and power is part of whether it is
    /// enabled — so switching a box is switching lights, chunks and all,
    /// and never a new scene.
    pub needs_power: bool,
    /// The map cell of the prop it came from, so the game can switch the
    /// light of the computer somebody sat down at.
    pub cell: Point,
}

impl Light {
    pub fn to_gpu(&self) -> [f32; 8] {
        [
            self.pos[0],
            self.pos[1],
            self.radius,
            if self.enabled { 1.0 } else { 0.0 },
            self.color[0],
            self.color[1],
            self.color[2],
            0.0,
        ]
    }

    /// The chunks its radius can reach, inclusive, clamped to the map.
    fn chunk_span(&self, chunks: (u32, u32)) -> (u32, u32, u32, u32) {
        let lo = |v: f32| (v - self.radius).floor().max(0.0) as u32 / CHUNK;
        let hi = |v: f32, n: u32| ((v + self.radius).ceil().max(0.0) as u32 / CHUNK).min(n - 1);
        (lo(self.pos[0]), lo(self.pos[1]), hi(self.pos[0], chunks.0), hi(self.pos[1], chunks.1))
    }
}

pub struct LightScene {
    /// Lightmap size in subtiles.
    pub width: u32,
    pub height: u32,
    /// One bit per subtile, row-major, set where light cannot pass.
    pub occlusion: Vec<u32>,
    /// One bit per **cell**, row-major, set where there is a ceiling.
    pub ceiling: Vec<u32>,
    /// The map's width in cells, which is what `ceiling` is indexed by.
    pub map_width: u32,
    pub lights: Vec<Light>,
    /// CSR: chunk `c`'s lights are `chunk_lights[chunk_offsets[c]..chunk_offsets[c + 1]]`.
    pub chunk_offsets: Vec<u32>,
    pub chunk_lights: Vec<u32>,
    dirty: Vec<bool>,
    /// Bumped by every change; the GPU side reports which one it has
    /// finished, so a reader knows the lightmap is up to date.
    generation: u64,
}

impl LightScene {
    /// The scene a map lights: its walls, its roofs, and every light on it —
    /// lit from the start if it is always on and `supply` gives it what it
    /// needs. A lamp with no power is in the scene, dark; a fire needs
    /// nothing. [`LightScene::apply_supply`] follows power changing.
    pub fn from_map(map: &Map, supply: &Supply) -> LightScene {
        let size = map.size();
        let width = size.width as u32 * SUBTILES as u32;
        let height = size.height as u32 * SUBTILES as u32;

        let mut occlusion = vec![0u32; (width * height).div_ceil(32) as usize];
        for cell in size.points() {
            if map.sight().is_passable(cell) {
                continue;
            }
            for sy in 0..SUBTILES {
                for sx in 0..SUBTILES {
                    let x = (cell.x * SUBTILES + sx) as u32;
                    let y = (cell.y * SUBTILES + sy) as u32;
                    let i = y * width + x;
                    occlusion[(i / 32) as usize] |= 1 << (i % 32);
                }
            }
        }

        let mut ceiling = vec![0u32; size.area().div_ceil(32)];
        for (i, cell) in size.points().enumerate() {
            debug_assert_eq!(size.index_of(cell), Some(i));
            // A window is open to the sky whatever roofs its wall, which is
            // what lets the sun in through it.
            let window = map.terrain(cell).is_some_and(TerrainId::lets_sun_in);
            if map.has_ceiling(cell) && !window {
                ceiling[i / 32] |= 1 << (i % 32);
            }
        }

        let to_subtiles = SUBTILES as f32 / PIXELS_PER_CELL as f32;
        let lights = map
            .objects(ObjectLayer::Props)
            .iter()
            .chain(map.objects(ObjectLayer::Lamps))
            .filter_map(|prop| {
                let e = emission(prop.kind.as_str())?;
                Some(Light {
                    pos: [prop.at.x as f32 * to_subtiles, prop.at.y as f32 * to_subtiles],
                    radius: e.radius * SUBTILES as f32,
                    enabled: e.always_on && supply.serves(prop.kind.as_str(), prop.cell()),
                    color: e.color,
                    switchable: !e.always_on,
                    needs_power: crate::map::utilities::needs(prop.kind.as_str())
                        == Some(crate::map::utilities::Utility::Power),
                    cell: prop.cell(),
                })
            })
            .collect();

        let mut scene = LightScene {
            width,
            height,
            occlusion,
            ceiling,
            map_width: size.width as u32,
            lights,
            chunk_offsets: Vec::new(),
            chunk_lights: Vec::new(),
            dirty: Vec::new(),
            generation: 0,
        };
        scene.rebuild_index();
        scene.dirty = vec![true; scene.chunk_count()];
        scene
    }

    pub fn chunks(&self) -> (u32, u32) {
        (self.width.div_ceil(CHUNK), self.height.div_ceil(CHUNK))
    }

    pub fn chunk_count(&self) -> usize {
        let (x, y) = self.chunks();
        (x * y) as usize
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Rebuild the chunk -> lights index. Only needed when lights are added,
    /// removed or moved; switching one on or off keeps it valid.
    fn rebuild_index(&mut self) {
        let chunks = self.chunks();
        let mut lists = vec![Vec::new(); self.chunk_count()];
        for (id, light) in self.lights.iter().enumerate() {
            let (x0, y0, x1, y1) = light.chunk_span(chunks);
            for cy in y0..=y1 {
                for cx in x0..=x1 {
                    lists[(cy * chunks.0 + cx) as usize].push(id as u32);
                }
            }
        }
        self.chunk_offsets = Vec::with_capacity(lists.len() + 1);
        self.chunk_lights.clear();
        self.chunk_offsets.push(0);
        for list in lists {
            self.chunk_lights.extend(list);
            self.chunk_offsets.push(self.chunk_lights.len() as u32);
        }
    }

    /// Switch a light; dirties the chunks it reaches if anything changed.
    /// Power changed — a box switched: every always-on light that needs
    /// power is lit exactly when its cell has it. A switchable one (a
    /// computer's screen) is left to whoever switches it, since without
    /// power nobody can be using it. Returns how many lights changed.
    pub fn apply_supply(&mut self, supply: &Supply) -> usize {
        let mut changed = 0;
        for id in 0..self.lights.len() {
            let light = &self.lights[id];
            if !light.needs_power || light.switchable {
                continue;
            }
            let powered = supply.is_live(crate::map::utilities::Network::Wiring, light.cell);
            changed += usize::from(self.set_enabled(id, powered));
        }
        changed
    }

    pub fn set_enabled(&mut self, id: usize, enabled: bool) -> bool {
        if self.lights[id].enabled == enabled {
            return false;
        }
        self.lights[id].enabled = enabled;
        let chunks = self.chunks();
        let (x0, y0, x1, y1) = self.lights[id].chunk_span(chunks);
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                self.dirty[(cy * chunks.0 + cx) as usize] = true;
            }
        }
        self.generation += 1;
        true
    }

    /// Dirty every chunk: a full re-bake. What the first bake is, and what
    /// `CROWD2X_LIGHT_STRESS` does every frame to time one.
    pub fn mark_all_dirty(&mut self) {
        self.dirty.iter_mut().for_each(|d| *d = true);
        self.generation += 1;
    }

    /// The dirty chunks, cleared. Ascending, so a dispatch is reproducible.
    pub fn take_dirty(&mut self) -> Vec<u32> {
        let mut out = Vec::new();
        for (i, d) in self.dirty.iter_mut().enumerate() {
            if std::mem::take(d) {
                out.push(i as u32);
            }
        }
        out
    }

    pub fn is_opaque(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return true;
        }
        let i = y as u32 * self.width + x as u32;
        self.occlusion[(i / 32) as usize] & (1 << (i % 32)) != 0
    }

    /// Whether subtile `(x, y)` has a ceiling over it. Only for subtiles on
    /// the map.
    pub fn is_roofed(&self, x: i32, y: i32) -> bool {
        let cell = (y / SUBTILES) as u32 * self.map_width + (x / SUBTILES) as u32;
        self.ceiling[(cell / 32) as usize] & (1 << (cell % 32)) != 0
    }

    /// The CPU reference: what the bake must write for texel `(x, y)` — the
    /// lights' RGB, and the sky factor from `sky`, which is
    /// [`LightScene::sky_field`] (computed once, it is the whole map). Kept
    /// line for line with `lighting.wgsl`, and the GPU's output is checked
    /// against it.
    pub fn evaluate(&self, x: u32, y: u32, sky: &[f32]) -> [f32; 4] {
        let [r, g, b] = self.lights_at(x, y);
        let s = sky[(y * self.width + x) as usize];
        [r, g, b, s * s]
    }

    /// The propagated sky, every subtile, before squaring: what the GPU's
    /// [`SKY_ITERATIONS`] passes leave behind, pass for pass.
    pub fn sky_field(&self) -> Vec<f32> {
        let (w, h) = (self.width as i32, self.height as i32);
        let mut src: Vec<f32> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| if self.is_roofed(x, y) { 0.0 } else { 1.0 })
            .collect();
        let mut dst = src.clone();
        for _ in 0..SKY_ITERATIONS {
            for y in 0..h {
                for x in 0..w {
                    dst[(y * w + x) as usize] = self.sky_step(&src, x, y);
                }
            }
            std::mem::swap(&mut src, &mut dst);
        }
        src
    }

    /// One subtile of one propagation pass.
    fn sky_step(&self, src: &[f32], x: i32, y: i32) -> f32 {
        let w = self.width as i32;
        if !self.is_roofed(x, y) {
            return 1.0;
        }
        let mut best = src[(y * w + x) as usize];
        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let (nx, ny) = (x + dx, y + dy);
                // Off the map is opaque, so this covers the edge too.
                if self.is_opaque(nx, ny) {
                    continue;
                }
                let cost = if dx != 0 && dy != 0 {
                    if self.is_opaque(x + dx, y) && self.is_opaque(x, y + dy) {
                        continue;
                    }
                    SKY_STEP_DIAGONAL
                } else {
                    SKY_STEP
                };
                best = best.max(src[(ny * w + nx) as usize] - cost);
            }
        }
        best.max(0.0)
    }

    /// The lights' share of [`LightScene::evaluate`].
    pub fn lights_at(&self, x: u32, y: u32) -> [f32; 3] {
        let chunks = self.chunks();
        let chunk = (y / CHUNK) * chunks.0 + x / CHUNK;
        let target = [x as f32 + 0.5, y as f32 + 0.5];
        let mut sum = [0.0f32; 3];
        let range = self.chunk_offsets[chunk as usize]..self.chunk_offsets[chunk as usize + 1];
        for &id in &self.chunk_lights[range.start as usize..range.end as usize] {
            let light = &self.lights[id as usize];
            if !light.enabled {
                continue;
            }
            let dx = target[0] - light.pos[0];
            let dy = target[1] - light.pos[1];
            let d2 = dx * dx + dy * dy;
            let r2 = light.radius * light.radius;
            if d2 >= r2 {
                continue;
            }
            if !self.visible(light.pos, x as i32, y as i32) {
                continue;
            }
            let f = 1.0 - d2 / r2;
            let a = f * f;
            for c in 0..3 {
                sum[c] += light.color[c] * a;
            }
        }
        sum
    }

    /// Whether the line from `from` to the centre of subtile `(tx, ty)`
    /// crosses an opaque subtile, the light's own and the target's excepted.
    ///
    /// A line through a corner steps diagonally, and is stopped there only
    /// when both subtiles beside the corner are opaque — light does not
    /// squeeze between two walls that meet at a corner, the rule
    /// `Map::sight` follows, and it grazes past one.
    fn visible(&self, from: [f32; 2], tx: i32, ty: i32) -> bool {
        let to = [tx as f32 + 0.5, ty as f32 + 0.5];
        let mut cx = from[0].floor() as i32;
        let mut cy = from[1].floor() as i32;
        let dx = to[0] - from[0];
        let dy = to[1] - from[1];
        let step_x = if dx > 0.0 { 1 } else { -1 };
        let step_y = if dy > 0.0 { 1 } else { -1 };
        let inv_x = if dx != 0.0 { 1.0 / dx.abs() } else { f32::INFINITY };
        let inv_y = if dy != 0.0 { 1.0 / dy.abs() } else { f32::INFINITY };
        let first_x = if dx > 0.0 { cx as f32 + 1.0 - from[0] } else { from[0] - cx as f32 };
        let first_y = if dy > 0.0 { cy as f32 + 1.0 - from[1] } else { from[1] - cy as f32 };
        let mut t_x = first_x * inv_x;
        let mut t_y = first_y * inv_y;
        let steps = (tx - cx).abs() + (ty - cy).abs();
        for _ in 0..steps {
            if (t_x - t_y).abs() < CORNER {
                if self.is_opaque(cx + step_x, cy) && self.is_opaque(cx, cy + step_y) {
                    return false;
                }
                cx += step_x;
                cy += step_y;
                t_x += inv_x;
                t_y += inv_y;
            } else if t_x < t_y {
                cx += step_x;
                t_x += inv_x;
            } else {
                cy += step_y;
                t_y += inv_y;
            }
            if cx == tx && cy == ty {
                return true;
            }
            if self.is_opaque(cx, cy) {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Object, ObjectKind, Size, FLOOR, WALL};

    fn fire_at(map: &mut Map, cell: Point) {
        map.add_object(
            ObjectLayer::Props,
            Object {
                at: Point::new(cell.x * PIXELS_PER_CELL + 24, cell.y * PIXELS_PER_CELL + 24),
                kind: ObjectKind::new("fire"),
            },
        );
    }

    fn sub(cell: i32) -> u32 {
        (cell * SUBTILES + SUBTILES / 2) as u32
    }

    #[test]
    fn the_day_is_white_the_night_is_dark_and_neither_jumps() {
        assert_eq!(ambient(12.0), [1.0; 3]);
        assert!(ambient(23.0)[0] < 0.2 && ambient(3.0) == ambient(23.0));
        assert!(ambient(20.0)[0] > ambient(20.0)[2], "dusk is warm");
        // Continuous: no step bigger than a few percent per world minute.
        let mut last = ambient(0.0);
        for minute in 1..=24 * 60 {
            let now = ambient(minute as f32 / 60.0);
            for c in 0..3 {
                assert!((now[c] - last[c]).abs() < 0.02, "{minute}: {last:?} -> {now:?}");
            }
            last = now;
        }
    }

    fn roofed_room(map: &mut Map, x0: i32, y0: i32, x1: i32, y1: i32, door: Point) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                map.set_ceiling(Point::new(x, y), true);
                if x == x0 || x == x1 || y == y0 || y == y1 {
                    map.set_terrain(Point::new(x, y), WALL);
                }
            }
        }
        map.set_terrain(door, FLOOR);
    }

    #[test]
    fn open_sky_is_whole_and_the_sun_comes_in_by_the_door_and_no_further() {
        let mut map = Map::new(Size::new(24, 20), FLOOR);
        // A room from (4, 2) to (16, 16), its door in the bottom wall at (10, 2).
        roofed_room(&mut map, 4, 2, 16, 16, Point::new(10, 2));
        let scene = LightScene::from_map(&map, &Supply::everywhere(map.size()));
        let field = scene.sky_field();
        let sky = |cell: Point| {
            let (x, y) = (cell.x * SUBTILES + 2, cell.y * SUBTILES + 2);
            scene.evaluate(x as u32, y as u32, &field)[3]
        };

        assert_eq!(sky(Point::new(1, 1)), 1.0, "the street");
        let inside_the_door = sky(Point::new(10, 3));
        let deeper = sky(Point::new(10, 5));
        let round_the_corner = sky(Point::new(12, 3));
        let the_back = sky(Point::new(10, 15));
        assert!(inside_the_door > deeper && deeper > 0.0, "{inside_the_door} {deeper}");
        assert!(inside_the_door < 1.0 && inside_the_door > 0.4, "{inside_the_door}");
        assert!(round_the_corner > 0.0, "it spills sideways, softly");
        assert_eq!(the_back, 0.0, "six cells is as far as it goes");
    }

    #[test]
    fn the_sun_comes_in_through_a_window_in_a_roofed_wall() {
        let window = TerrainId::from_name("window").unwrap();
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        roofed_room(&mut map, 2, 2, 6, 6, Point::new(2, 2));
        map.set_terrain(Point::new(2, 2), WALL);
        map.set_terrain(Point::new(4, 2), window);
        assert!(map.has_ceiling(Point::new(4, 2)), "the wall stays roofed");
        let scene = LightScene::from_map(&map, &Supply::everywhere(map.size()));
        let field = scene.sky_field();
        let sky = |x: i32, y: i32| scene.evaluate((x * SUBTILES + 2) as u32, (y * SUBTILES + 2) as u32, &field)[3];
        assert!(sky(4, 3) > 0.4, "just inside the window: {}", sky(4, 3));
        assert!(sky(4, 5) > 0.0 && sky(4, 5) < sky(4, 3));
        assert_eq!(sky(3, 2), 0.0, "the wall beside it is still shade");
    }

    #[test]
    fn a_closed_room_has_no_sky_however_close_the_street() {
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        roofed_room(&mut map, 2, 2, 6, 6, Point::new(2, 2));
        map.set_terrain(Point::new(2, 2), WALL);
        let scene = LightScene::from_map(&map, &Supply::everywhere(map.size()));
        let field = scene.sky_field();
        for y in 3..6 {
            for x in 3..6 {
                let (sx, sy) = ((x * SUBTILES + 2) as u32, (y * SUBTILES + 2) as u32);
                assert_eq!(scene.evaluate(sx, sy, &field)[3], 0.0, "({x}, {y})");
            }
        }
    }

    #[test]
    fn the_sky_does_not_squeeze_between_walls_meeting_at_a_corner() {
        // A roofed room whose only opening is a pinch: two wall cells touching
        // at a corner, open sky beyond.
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        for y in 0..10 {
            for x in 0..10 {
                map.set_ceiling(Point::new(x, y), x < 5 && y < 5);
            }
        }
        for i in 0..5 {
            map.set_terrain(Point::new(5, i), WALL);
            map.set_terrain(Point::new(i, 5), WALL);
        }
        map.set_terrain(Point::new(5, 5), FLOOR);
        // (4, 5) and (5, 4) are wall; (5, 5) is open, diagonal from (4, 4).
        let scene = LightScene::from_map(&map, &Supply::everywhere(map.size()));
        let field = scene.sky_field();
        assert_eq!(field[(22 * scene.width + 22) as usize], 0.0);
    }

    #[test]
    fn a_ceiling_lamp_lights_its_room_and_not_the_street_behind_the_wall() {
        let mut map = Map::new(Size::new(16, 12), FLOOR);
        roofed_room(&mut map, 2, 2, 10, 9, Point::new(6, 2));
        map.add_object(
            ObjectLayer::Lamps,
            Object { at: Point::new(6 * 48 + 24, 6 * 48 + 24), kind: ObjectKind::new("ceiling lamp") },
        );
        let scene = LightScene::from_map(&map, &Supply::everywhere(map.size()));
        assert_eq!(scene.lights.len(), 1);
        assert!(scene.lights_at(sub(6), sub(7))[0] > 0.5, "under the lamp");
        assert_eq!(scene.lights_at(sub(6), sub(10))[0], 0.0, "beyond the top wall");

        // The same lamp with no wiring to it is not a light.
        let mut dark = LightScene::from_map(&map, &Supply::from_map(&map));
        assert!(!dark.lights[0].enabled, "a lamp with no power lights nothing");
        assert_eq!(dark.lights_at(sub(6), sub(7))[0], 0.0);
        crate::map::utilities::serve_everything(&mut map, Point::new(0, 0), Point::new(15, 0));
        let supply = Supply::from_map(&map);
        let wired = LightScene::from_map(&map, &supply);
        assert!(wired.lights[0].enabled, "until it is wired up");
        // ...and a scene that was dark lights up when the power comes on,
        // and goes dark again with its box switched off.
        let generation = dark.generation();
        assert_eq!(dark.apply_supply(&supply), 1);
        assert!(dark.lights[0].enabled && dark.generation() > generation);
        assert_eq!(dark.apply_supply(&supply), 0, "nothing more to change");
        let boxes: Vec<Point> = map
            .size()
            .points()
            .filter(|&cell| map.grid(crate::map::GridLayer::Power, cell) == crate::map::grid::power::BOX)
            .collect();
        assert_eq!(dark.apply_supply(&Supply::with_boxes_off(&map, &boxes)), 1);
        assert!(!dark.lights[0].enabled);
    }

    #[test]
    fn a_lightmap_is_five_subtiles_to_a_cell() {
        let scene = { let map = Map::new(Size::new(10, 4), FLOOR); LightScene::from_map(&map, &Supply::everywhere(map.size())) };
        assert_eq!((scene.width, scene.height), (50, 20));
    }

    #[test]
    fn light_falls_off_to_nothing_at_its_radius() {
        let mut map = Map::new(Size::new(20, 20), FLOOR);
        fire_at(&mut map, Point::new(10, 10));
        let scene = LightScene::from_map(&map, &Supply::everywhere(map.size()));
        let near = scene.lights_at(sub(10), sub(11))[0];
        let far = scene.lights_at(sub(10), sub(15))[0];
        assert!(near > far && far > 0.0, "{near} {far}");
        assert_eq!(scene.lights_at(sub(10), sub(18))[..3], [0.0; 3]);
    }

    #[test]
    fn a_wall_casts_a_shadow_and_its_face_is_lit() {
        let mut map = Map::new(Size::new(20, 20), FLOOR);
        for y in 0..20 {
            map.set_terrain(Point::new(12, y), WALL);
        }
        fire_at(&mut map, Point::new(10, 10));
        let scene = LightScene::from_map(&map, &Supply::everywhere(map.size()));
        let face = (12 * SUBTILES) as u32;
        assert!(scene.lights_at(face, sub(10))[0] > 0.0, "the face is lit");
        assert_eq!(scene.lights_at(face + 1, sub(10)), [0.0; 3], "inside the wall is not");
        assert_eq!(scene.lights_at(sub(14), sub(10))[..3], [0.0; 3], "behind the wall is dark");
    }

    #[test]
    fn light_does_not_squeeze_between_walls_meeting_at_a_corner() {
        // Two wall cells touching only at a corner, with the fire on the
        // diagonal through it: every subtile of the far quadrant is behind
        // the pinch, and lines to it cross the corner exactly.
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        map.set_terrain(Point::new(5, 6), WALL);
        map.set_terrain(Point::new(6, 5), WALL);
        // Walls closing off the rest of the far quadrant, so only the corner
        // could let light in.
        for i in 6..12 {
            map.set_terrain(Point::new(5, i), WALL);
            map.set_terrain(Point::new(i, 5), WALL);
        }
        fire_at(&mut map, Point::new(3, 3));
        let scene = LightScene::from_map(&map, &Supply::everywhere(map.size()));
        // The fire is at subtile (17.5, 17.5); (32, 32) is on its diagonal,
        // through the corner at (30, 30).
        assert_eq!(scene.lights_at(32, 32), [0.0; 3]);
        assert!(scene.lights_at(28, 28)[0] > 0.0, "this side of the pinch is lit");
    }

    /// Which side of a corner the walk tries first must not matter: with the
    /// two crossings tied, rounding decides it, and the CPU and the GPU round
    /// differently.
    #[test]
    fn grazing_one_wall_corner_is_the_same_whichever_side_the_wall_is_on() {
        let lit_past = |wall: Point| {
            let mut map = Map::new(Size::new(12, 12), FLOOR);
            map.set_terrain(wall, WALL);
            fire_at(&mut map, Point::new(3, 3));
            LightScene::from_map(&map, &Supply::everywhere(map.size())).lights_at(32, 32)
        };
        let beside = lit_past(Point::new(6, 5));
        let above = lit_past(Point::new(5, 6));
        assert_eq!(beside, above);
        assert!(beside[0] > 0.0, "a single corner is grazed past");
    }

    #[test]
    fn switching_a_light_dirties_only_the_chunks_it_reaches() {
        let mut map = Map::new(Size::new(64, 64), FLOOR);
        map.add_object(
            ObjectLayer::Props,
            Object { at: Point::new(3 * 48 + 24, 3 * 48 + 24), kind: ObjectKind::new("computer") },
        );
        let mut scene = LightScene::from_map(&map, &Supply::everywhere(map.size()));
        assert_eq!(scene.take_dirty().len(), scene.chunk_count(), "the first bake is everything");
        assert!(scene.take_dirty().is_empty());
        assert!(scene.set_enabled(0, true));
        assert!(!scene.set_enabled(0, true), "no change, nothing dirtied");
        let dirty = scene.take_dirty();
        assert!(!dirty.is_empty() && dirty.len() <= 4, "{dirty:?}");
    }

    #[test]
    fn every_chunk_lists_every_light_that_reaches_it() {
        let mut map = Map::new(Size::new(40, 40), FLOOR);
        for i in 0..12 {
            fire_at(&mut map, Point::new((i * 7) % 40, (i * 13) % 40));
        }
        let scene = LightScene::from_map(&map, &Supply::everywhere(map.size()));
        // Brute force: every lit texel's lights must be in its chunk's list,
        // which `evaluate` relies on.
        for (id, light) in scene.lights.iter().enumerate() {
            for y in (0..scene.height).step_by(3) {
                for x in (0..scene.width).step_by(3) {
                    let d = ((x as f32 + 0.5 - light.pos[0]).powi(2)
                        + (y as f32 + 0.5 - light.pos[1]).powi(2))
                    .sqrt();
                    if d < light.radius {
                        let c = ((y / CHUNK) * scene.chunks().0 + x / CHUNK) as usize;
                        let list = &scene.chunk_lights
                            [scene.chunk_offsets[c] as usize..scene.chunk_offsets[c + 1] as usize];
                        assert!(list.contains(&(id as u32)), "light {id} missing from chunk {c}");
                    }
                }
            }
        }
    }
}
