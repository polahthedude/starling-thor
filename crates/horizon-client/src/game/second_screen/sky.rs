//! The panel's sky strip, drawn with the resource packs' own textures where vanilla has them:
//! the sky's colours, the sun and the moon in its phase on their arc with the time of day,
//! stars at night, the clouds drifting, rain or snow as the biome has it; the Nether's ash,
//! the End's sky; bubbles under water; and along its bottom the coordinates and biome.
use super::ambience::{Mood, Scene, lerp3, packed_alpha};
use super::*;
use horizon_client::hud::Rect;
use std::f32::consts::{PI, TAU};

/// The moon's phases in vanilla's order (`dayTime / 24000 % 8`).
const MOON_PHASES: [&str; 8] = [
    "full_moon",
    "waning_gibbous",
    "third_quarter",
    "waning_crescent",
    "new_moon",
    "waxing_crescent",
    "first_quarter",
    "waxing_gibbous",
];

/// A steady pseudo-random 0 to 1 for item `i` of a kind (`salt`): where a star, a raindrop,
/// a speck of ash goes.
fn hash(i: u32, salt: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B1) ^ salt.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^= x >> 15;
    (x & 0xFFFF) as f32 / 65535.0
}

/// A rectangle shaded from its corners' colours (top left, top right, bottom right, bottom
/// left), as `HudBatch::fill_gradient` is from its edges'.
fn fill_corners(
    batch: &mut HudBatch,
    atlas: &horizon_client::hud::HudAtlas,
    [x, y, w, h]: [f32; 4],
    colors: [u32; 4],
) {
    // `fill_gradient`'s two triangles, corners 0 1 2 and 0 2 3, recoloured.
    let start = batch.vertices.len();
    batch.fill_gradient(atlas, (x, y, w, h), 0, 0);
    for (v, corner) in batch.vertices[start..].iter_mut().zip([0, 1, 2, 0, 2, 3]) {
        v.color = colors[corner];
    }
}

/// The part `[a, b)` (0 to 1, across and down) of sprite `r`.
fn sub_rect(r: Rect, [u0, v0, u1, v1]: [f32; 4]) -> Rect {
    let mut q = r;
    let (du, dv) = (r.u1 - r.u0, r.v1 - r.v0);
    (q.u0, q.u1) = (r.u0 + du * u0, r.u0 + du * u1);
    (q.v0, q.v1) = (r.v0 + dv * v0, r.v0 + dv * v1);
    q
}

impl Game<'_> {
    /// The biome the player stands in, as `minecraft:<name>`.
    fn panel_biome(&self) -> Option<String> {
        let p = self.player.as_ref()?;
        let at = [p.position.x, p.position.y, p.position.z].map(|v| v.floor() as i32);
        let chunk = self.world.chunks.get(&(at[0] >> 4, at[2] >> 4))?;
        let id = chunk.biome(at[0], at[1], at[2]);
        Some(world_biomes!(self).get(id).name.clone())
    }

    /// The sky strip `rect`: what the world's sky shows now, clipped to it; the sun, the
    /// moon and the clouds in `open`, its part between the hotbar and the info row, and the
    /// dotted path the sun and moon follow when `path` (not behind vanilla's screens).
    pub(super) fn draw_panel_sky(
        &self,
        batch: &mut HudBatch,
        rect: [f32; 4],
        open: [f32; 4],
        path: bool,
    ) {
        let [x, y, w, h] = rect;
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let Some((mood, scene)) = self.world_sky() else {
            return;
        };
        // The colours eased as the backdrop's are, so the two change together.
        let mood = self.eased_mood().unwrap_or(mood);
        let start = batch.vertices.len();
        let atlas = &self.gui.atlas;
        let top = mood.sky.map(|v| v * 0.92);
        let bottom = lerp3(mood.sky, mood.fog, 0.65);
        batch.fill_gradient(
            atlas,
            (x, y, w, h),
            packed_alpha(top, 1.0),
            packed_alpha(bottom, 1.0),
        );
        let t = self.started.elapsed().as_secs_f32();
        if scene.water {
            self.sky_bubbles(batch, rect, t);
        } else if scene.lava {
            self.sky_specks(batch, rect, t, [1.0, 0.6, 0.2], -14.0);
        } else {
            match scene.dim {
                Dim::Nether => {
                    self.sky_specks(batch, rect, t, lerp3(mood.fog, [1.0; 3], 0.5), -4.0)
                }
                Dim::End => {
                    self.sky_end(batch, rect);
                    self.sky_stars(batch, rect, t, 0.8);
                }
                _ => {
                    self.sky_stars(batch, rect, t, scene.stars * (1.0 - scene.rain));
                    self.sky_celestial(batch, open, &scene, path);
                    let horizon = open[1] + open[3];
                    self.sky_clouds(batch, rect, horizon, t, &scene, &mood);
                    if scene.rain > 0.01 {
                        match scene.snow {
                            Some(true) => self.sky_snow(batch, rect, t, scene.rain),
                            Some(false) => self.sky_rain(batch, rect, t, scene.rain),
                            None => {}
                        }
                    }
                }
            }
        }
        batch.clip_since(start, x, y, x + w, y + h);
    }

    /// Stars, `brightness` 0 (day) to 1, each twinkling a little.
    fn sky_stars(&self, batch: &mut HudBatch, [x, y, w, h]: [f32; 4], t: f32, brightness: f32) {
        if brightness <= 0.01 {
            return;
        }
        let atlas = &self.gui.atlas;
        for i in 0..70 {
            let twinkle = 0.65 + 0.35 * (t * (1.0 + hash(i, 3)) + hash(i, 4) * TAU).sin();
            let alpha = brightness * twinkle * (0.4 + 0.6 * hash(i, 5));
            let (sx, sy) = (x + hash(i, 1) * w, y + hash(i, 2) * h);
            batch.fill(
                atlas,
                sx.floor(),
                sy.floor(),
                1.0,
                1.0,
                packed_alpha([1.0; 3], alpha),
            );
        }
    }

    /// The sun and the moon on one dotted arc over the open sky, its ends on the horizon
    /// (`open`'s bottom, the frame's top): the sun rising at the left at tick 0, overhead at
    /// noon (6000), setting at the right at 12000, the moon opposite; at sunrise and sunset
    /// the sky's glow rises from the horizon on the sun's side. The sun's and the moon's
    /// textures are added to the sky, as vanilla draws them (their black is nothing).
    fn sky_celestial(
        &self,
        batch: &mut HudBatch,
        [x, y, w, h]: [f32; 4],
        scene: &Scene,
        path: bool,
    ) {
        const SUN: f32 = 32.0;
        const MOON: f32 = 24.0;
        let atlas = &self.gui.atlas;
        let day = scene.day_time.rem_euclid(24000) as f32 / 24000.0;
        let (cx, horizon) = ((x + w / 2.0).floor(), y + h);
        // The arc keeps the panel's proportions (at most 0.6 as tall as half its width) in
        // any window, a tall screen's whole height included.
        let rx = (w / 2.0 - SUN / 2.0).floor();
        let ry = (h - SUN / 2.0 - 2.0).min((rx * 0.6).floor()).max(4.0);
        let at = |angle: f32| [cx - rx * angle.cos(), horizon - ry * angle.sin()];
        // The glow: strongest at the horizon on the sun's side.
        let [gr, gg, gb, ga] = scene.sunrise;
        if ga > 0.01 {
            let strength = ga * (1.0 - scene.rain * 0.7);
            let near = packed_alpha([gr, gg, gb], strength);
            let far = packed_alpha([gr, gg, gb], strength * 0.25);
            let clear = packed_alpha([gr, gg, gb], 0.0);
            let rising = at(day * TAU)[0] < cx;
            let (left, right) = if rising { (near, far) } else { (far, near) };
            let glow = [x, (y + h * 0.25).floor(), w, (h * 0.75).ceil()];
            fill_corners(batch, atlas, glow, [clear, clear, right, left]);
        }
        // The path: a dot every 4 pixels or so, every fourth a 2-pixel bead, fading into the
        // horizon at both ends; the way already come brighter than the way ahead; warm by
        // day, cool by night; parted around the sun and the moon.
        let sun_angle = (day * TAU).rem_euclid(TAU);
        let up = sun_angle < PI;
        // The body above the horizon now, and how far along the arc it is.
        let (body, size) = if up {
            (sun_angle, SUN)
        } else {
            (sun_angle - PI, MOON)
        };
        let tint = if up {
            [1.0, 0.95, 0.82]
        } else {
            [0.78, 0.85, 1.0]
        };
        let dots = if path {
            (PI * (rx + ry) / 2.0 / 4.0).max(8.0) as u32
        } else {
            0
        };
        let [bx, by] = at(body);
        let weather = 1.0 - scene.rain * 0.6;
        for i in 1..dots {
            let angle = PI * i as f32 / dots as f32;
            let [px, py] = at(angle);
            if (px - bx).hypot(py - by) < size * 0.5 + 3.0 {
                continue;
            }
            let fade = angle.sin().sqrt();
            let come = if angle <= body { 0.55 } else { 0.25 };
            let color = packed_alpha(tint, come * fade * weather);
            let bead = i % 4 == 0;
            let (d, off) = if bead { (2.0, 0.5) } else { (1.0, 0.0) };
            batch.fill(atlas, (px - off).floor(), (py - off).floor(), d, d, color);
        }
        let alpha = 1.0 - scene.rain * 0.85;
        let phase = (scene.day_time.div_euclid(24000)).rem_euclid(8) as usize;
        let moon = format!("environment/celestial/moon/{}", MOON_PHASES[phase]);
        for (angle, sprite, size) in [
            (day * TAU, "environment/celestial/sun", SUN),
            (day * TAU + PI, moon.as_str(), MOON),
        ] {
            let [px, py] = at(angle);
            if py - size / 2.0 > horizon {
                continue;
            }
            if let Some(r) = atlas.sprites.get(sprite).copied() {
                let start = batch.vertices.len();
                let color = packed_alpha([alpha; 3], 1.0);
                batch.rect_uv(
                    (px - size / 2.0).floor(),
                    (py - size / 2.0).floor(),
                    size,
                    size,
                    r,
                    color,
                );
                batch.blend_since(start, horizon_client::hud::Blend::Additive);
            }
        }
    }

    /// The pack's cloud map drifting across the whole window from its top, four pixels a
    /// cloud cell, in the clouds' colour, row by row fainter down to nothing at `horizon`
    /// (the arc's foot), none below it.
    fn sky_clouds(
        &self,
        batch: &mut HudBatch,
        [x, y, w, _]: [f32; 4],
        horizon: f32,
        t: f32,
        scene: &Scene,
        mood: &Mood,
    ) {
        let Some(r) = self.gui.atlas.sprites.get("environment/clouds").copied() else {
            return;
        };
        const CELL: f32 = 4.0;
        const SIZE: f32 = 256.0;
        let depth = horizon - y;
        if depth <= 0.0 {
            return;
        }
        let rows = (depth / CELL).ceil() as u32;
        let cols = (w / CELL).ceil();
        // The first row of the map shown, and how far it has drifted (cells).
        let first = 96.0;
        let drift = (t * 0.35).rem_euclid(SIZE);
        let [cr, cg, cb, ca] = scene.clouds;
        let shade = 0.55 + 0.45 * mood.daylight;
        let rgb = [cr * shade, cg * shade, cb * shade];
        for i in 0..rows {
            let top = y + i as f32 * CELL;
            let fade = 1.0 - ((top + CELL / 2.0 - y) / depth).clamp(0.0, 1.0);
            if fade <= 0.01 {
                break;
            }
            let color = packed_alpha(rgb, ca * 0.85 * fade);
            let row = (first + i as f32).rem_euclid(SIZE);
            let height = CELL.min(horizon - top);
            let mut shown = 0.0;
            while shown < cols {
                let from = (drift + shown).rem_euclid(SIZE);
                let run = (SIZE - from).min(cols - shown);
                let part = sub_rect(
                    r,
                    [
                        from / SIZE,
                        row / SIZE,
                        (from + run) / SIZE,
                        (row + height / CELL) / SIZE,
                    ],
                );
                batch.rect_uv(x + shown * CELL, top, run * CELL, height, part, color);
                shown += run;
            }
        }
    }

    /// Rain: falling streaks, as many as the rain is heavy.
    fn sky_rain(&self, batch: &mut HudBatch, [x, y, w, h]: [f32; 4], t: f32, level: f32) {
        let atlas = &self.gui.atlas;
        let color = packed_alpha([0.7, 0.78, 1.0], 0.55 * level);
        for i in 0..(80.0 * level) as u32 {
            let speed = 110.0 + 50.0 * hash(i, 7);
            let fall = (hash(i, 8) * (h + 6.0) + t * speed).rem_euclid(h + 6.0) - 6.0;
            let sx = x + (hash(i, 6) * w - fall * 0.15).rem_euclid(w);
            batch.fill(atlas, sx.floor(), (y + fall).floor(), 1.0, 4.0, color);
        }
    }

    /// Snow: slow flakes, swaying.
    fn sky_snow(&self, batch: &mut HudBatch, [x, y, w, h]: [f32; 4], t: f32, level: f32) {
        let atlas = &self.gui.atlas;
        for i in 0..(70.0 * level) as u32 {
            let speed = 10.0 + 8.0 * hash(i, 10);
            let fall = (hash(i, 11) * (h + 2.0) + t * speed).rem_euclid(h + 2.0) - 2.0;
            let sway = (t * (0.8 + hash(i, 12)) + hash(i, 13) * TAU).sin() * 3.0;
            let sx = x + (hash(i, 9) * w + sway).rem_euclid(w);
            let size = if hash(i, 14) > 0.7 { 2.0 } else { 1.0 };
            let color = packed_alpha([1.0; 3], 0.8 * level);
            batch.fill(atlas, sx.floor(), (y + fall).floor(), size, size, color);
        }
    }

    /// Specks drifting up (`rise` pixels a second, negative to sink): the Nether's ash, the
    /// embers in lava.
    fn sky_specks(
        &self,
        batch: &mut HudBatch,
        [x, y, w, h]: [f32; 4],
        t: f32,
        color: [f32; 3],
        rise: f32,
    ) {
        let atlas = &self.gui.atlas;
        for i in 0..45 {
            let speed = rise * (0.5 + hash(i, 16));
            let up = (hash(i, 17) * h - t * speed).rem_euclid(h);
            let sway = (t * 0.6 + hash(i, 18) * TAU).sin() * 4.0;
            let sx = x + (hash(i, 15) * w + sway).rem_euclid(w);
            let alpha = 0.35 + 0.4 * hash(i, 19);
            batch.fill(
                atlas,
                sx.floor(),
                (y + up).floor(),
                1.0,
                1.0,
                packed_alpha(color, alpha),
            );
        }
    }

    /// Bubbles rising, as under water.
    fn sky_bubbles(&self, batch: &mut HudBatch, [x, y, w, h]: [f32; 4], t: f32) {
        let atlas = &self.gui.atlas;
        let color = packed_alpha([0.75, 0.9, 1.0], 0.6);
        for i in 0..24 {
            let speed = 12.0 + 10.0 * hash(i, 21);
            let up = (hash(i, 22) * (h + 4.0) + t * speed).rem_euclid(h + 4.0);
            let wobble = (t * 2.0 + hash(i, 23) * TAU).sin() * 1.5;
            let (bx, by) = ((x + hash(i, 20) * w + wobble).floor(), (y + h - up).floor());
            let size = if hash(i, 24) > 0.6 { 2.0 } else { 1.0 };
            batch.fill(atlas, bx, by, size, size, color);
        }
    }

    /// The End's sky texture, tiled.
    fn sky_end(&self, batch: &mut HudBatch, [x, y, w, h]: [f32; 4]) {
        let Some(r) = self.gui.atlas.sprites.get("environment/end_sky").copied() else {
            return;
        };
        let tile = 32.0;
        let mut ty = y;
        while ty < y + h {
            let mut tx = x;
            while tx < x + w {
                batch.rect_uv(tx, ty, tile, tile, r, rgba(255, 255, 255, 110));
                tx += tile;
            }
            ty += tile;
        }
    }

    /// The coordinates at the left and the biome's name at the right, as bare text on the
    /// frame (vanilla's container text); the name shrinks rather than meet the coordinates.
    pub(super) fn draw_panel_info(&self, batch: &mut HudBatch, rect: [f32; 4]) {
        let [x, y, w, h] = rect;
        let atlas = &self.gui.atlas;
        let Some(p) = &self.player else {
            return;
        };
        let at = [p.position.x, p.position.y, p.position.z].map(|v| v.floor() as i64);
        let coords = format!("X {}  Y {}  Z {}", at[0], at[1], at[2]);
        let ty = (y + (h - 8.0) / 2.0).floor();
        let left = batch.text(atlas, &coords, x, ty, super::menu::TEXT, false);
        let Some(biome) = self.panel_biome() else {
            return;
        };
        let path = biome.replace(':', ".");
        let fallback = biome
            .rsplit(':')
            .next()
            .unwrap_or(&biome)
            .split('_')
            .map(|word| {
                let mut c = word.chars();
                c.next()
                    .map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
            })
            .collect::<Vec<_>>()
            .join(" ");
        let name = self.translate(&format!("biome.{path}"), &fallback);
        let atlas = &self.gui.atlas;
        let room = w - left - 8.0;
        let nw = HudBatch::text_width(atlas, &name).max(1.0);
        let k = (room / nw).min(1.0);
        if k < 0.5 {
            return;
        }
        let (nx, mid) = ((x + w - nw * k).floor(), ty + 4.0);
        let start = batch.vertices.len();
        batch.text(atlas, &name, nx, ty, 0xFF70_7070, false);
        if k < 1.0 {
            hud::transform_since(batch, start, [nx, mid], k, [0.0, 0.0]);
        }
    }
}
