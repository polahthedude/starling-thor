//! The second screen's backdrop follows the world on the window: its colours come from the
//! sky the window draws (`EnvironmentAttributeProbe`'s biome-blended sky and fog on the day
//! timeline, darkened by rain and thunder), the dimension's, and the water or lava the camera
//! is in, eased so a change (dusk, a portal) fades rather than jumps. Light layouts (the panel,
//! the inventory, crafting, creative) keep a light ground for their dark text, tinted; dark
//! ones (containers, the screens moved whole) take a deep version of the sky.
use super::*;
use std::time::Instant;

/// What the panel's sky window shows besides the colours: this frame's, not eased.
#[derive(Clone, Copy)]
pub(super) struct Scene {
    pub day_time: i64,
    /// `star_brightness`, 0 by day.
    pub stars: f32,
    /// The clouds' colour and opacity.
    pub clouds: [f32; 4],
    /// The sunrise and sunset glow's colour, its strength in the alpha (0 most of the day).
    pub sunrise: [f32; 4],
    /// How hard it rains or snows, 0 to 1.
    pub rain: f32,
    /// Where the player is it snows (Some(true)), rains (Some(false)), or neither (None).
    pub snow: Option<bool>,
    pub dim: Dim,
    pub water: bool,
    pub lava: bool,
}

/// The colours the backdrop is made from, before a layout's style.
#[derive(Clone, Copy)]
pub(super) struct Mood {
    /// Toward the top: the sky (in the Nether and the End, mostly their fog).
    pub sky: [f32; 3],
    /// Toward the bottom: the horizon's fog.
    pub fog: [f32; 3],
    /// 0 at night to 1 at noon (or a dimension's constant light).
    pub daylight: f32,
}

thread_local! {
    /// The eased mood and when it was last eased.
    static EASED: RefCell<Option<(Mood, Instant)>> = const { RefCell::new(None) };
}

/// How quickly the backdrop follows a change, per second.
const EASE: f32 = 2.5;

pub(super) fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

fn luminance(c: [f32; 3]) -> f32 {
    c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722
}

fn packed(c: [f32; 3]) -> u32 {
    packed_alpha(c, 1.0)
}

/// `c` at opacity `alpha` (0 to 1), as the HUD's vertex colours.
pub(super) fn packed_alpha(c: [f32; 3], alpha: f32) -> u32 {
    let [r, g, b] = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    rgba(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The backdrop's top and bottom colours under `mood`, for a light layout or a dark one.
fn backdrop(mood: Mood, light: bool) -> ([f32; 3], [f32; 3]) {
    if light {
        // Vanilla's container grey, tinted, dimmed toward night but never below the light
        // ground the layouts' dark text needs.
        let grey = [198.0 / 255.0; 3];
        let dim = 0.72 + 0.28 * mood.daylight;
        let lift = |c: [f32; 3]| {
            let c = c.map(|v| v * dim);
            let l = luminance(c);
            if l < 0.5 {
                c.map(|v| v * 0.5 / l.max(0.01))
            } else {
                c
            }
        };
        (
            lift(lerp3(grey, mood.sky, 0.38)),
            lift(lerp3(grey, mood.fog, 0.28)),
        )
    } else {
        (
            mood.sky.map(|v| v * 0.32 + 0.025),
            mood.fog.map(|v| v * 0.15 + 0.02),
        )
    }
}

impl Game<'_> {
    /// The world's mood this frame; None without a world (the title, its screens).
    fn world_mood(&self) -> Option<Mood> {
        self.world_sky().map(|(mood, _)| mood)
    }

    /// The world's mood and scene this frame; None without a world (the title, its screens).
    pub(super) fn world_sky(&self) -> Option<(Mood, Scene)> {
        if self.title.is_some() {
            return None;
        }
        let p = self.player.as_ref()?;
        let eye = [
            p.position.x,
            p.position.y + f64::from(p.dimensions.eye_height),
            p.position.z,
        ];
        let environment = self.dim_environment();
        let biomes = world_biomes!(self);
        let weights = {
            let chunks = &self.world.chunks;
            let plains = biomes.id("minecraft:plains");
            horizon_client::biome::gaussian_weights(eye, |qx, qy, qz| {
                chunks
                    .get(&(qx >> 2, qz >> 2))
                    .map_or(plains, |c| c.biome(qx * 4, qy * 4, qz * 4))
            })
        };
        let blend = |pick: fn(&horizon_client::biome::BiomeInfo) -> Option<u32>, base: [f32; 3]| {
            horizon_client::biome::blend_rgb(&weights, |b| {
                pick(biomes.get(b)).map_or(base, horizon_client::biome::rgb)
            })
        };
        let biome_sky = blend(
            |b| b.sky,
            environment.base_rgb("minecraft:visual/sky_color"),
        );
        let biome_fog = blend(
            |b| b.fog,
            environment.base_rgb("minecraft:visual/fog_color"),
        );
        let mut sky = environment.sample(self.world.day_time, Some(biome_sky), Some(biome_fog));
        let (rain, thunder) = if self.dim == Dim::Overworld {
            self.world.weather.levels(self.clock.fraction() as f32)
        } else {
            (0.0, 0.0)
        };
        horizon_client::environment::apply_weather(&mut sky, rain, thunder);
        let water = self.ambience.was_underwater();
        let lava = !water
            && matches!(
                self.camera_fluid(eye),
                horizon_client::camera::CameraFluid::Lava
            );
        let feet = [p.position.x, p.position.y, p.position.z].map(|v| v.floor() as i32);
        let snow = self
            .world
            .chunks
            .get(&(feet[0] >> 4, feet[2] >> 4))
            .and_then(|c| {
                let biome = biomes.get(c.biome(feet[0], feet[1], feet[2]));
                biome.snows_at(feet[0], feet[1], feet[2])
            });
        let scene = Scene {
            day_time: self.world.day_time,
            stars: sky.star_brightness,
            clouds: sky.cloud_color,
            sunrise: sky.sunrise_color,
            rain,
            snow,
            dim: self.dim,
            water,
            lava,
        };
        // Under water or in lava, their fog is all there is.
        if water {
            let water = blend(|b| Some(b.water_fog), [0.02, 0.02, 0.2]);
            let mood = Mood {
                sky: lerp3(water, [0.2, 0.5, 0.9], 0.35),
                fog: water,
                daylight: 0.5,
            };
            return Some((mood, scene));
        }
        if lava {
            let mood = Mood {
                sky: [0.9, 0.35, 0.05],
                fog: [0.6, 0.1, 0.0],
                daylight: 0.6,
            };
            return Some((mood, scene));
        }
        let mood = if self.dim == Dim::Overworld {
            let daylight = luminance(sky.sky_color) / luminance(biome_sky).max(0.05);
            Mood {
                sky: sky.sky_color,
                fog: sky.fog_color,
                daylight: daylight.clamp(0.0, 1.0),
            }
        } else {
            // The Nether's and the End's skies are dark or flat: their fog has the colour
            // (the Nether's biomes each their own).
            Mood {
                sky: lerp3(sky.sky_color, sky.fog_color, 0.7),
                fog: sky.fog_color,
                daylight: 0.55,
            }
        };
        Some((mood, scene))
    }

    /// The mood eased toward this frame's; None (and forgotten) without a world.
    pub(super) fn eased_mood(&self) -> Option<Mood> {
        let now = Instant::now();
        let target = self.world_mood();
        EASED.with_borrow_mut(|eased| {
            let Some(target) = target else {
                *eased = None;
                return None;
            };
            let mood = match *eased {
                Some((was, at)) => {
                    let k = 1.0 - (-EASE * now.duration_since(at).as_secs_f32()).exp();
                    Mood {
                        sky: lerp3(was.sky, target.sky, k),
                        fog: lerp3(was.fog, target.fog, k),
                        daylight: was.daylight + (target.daylight - was.daylight) * k,
                    }
                }
                None => target,
            };
            *eased = Some((mood, now));
            Some(mood)
        })
    }

    /// Put the world's backdrop under everything in `batch` for a layout drawn on `clear`,
    /// and give the colour to clear to: under the light panel grey a tinted gradient; under
    /// the dark menu ground (containers, the beacon, trading, the screens moved down from the
    /// window) the panel's living sky, dimmed so vanilla's white text stays clear. Other
    /// grounds (the death screen's red, the title's) are kept as they are.
    pub(super) fn ambient_backdrop(&self, batch: &mut HudBatch, clear: wgpu::Color) -> wgpu::Color {
        let light = clear == BACKGROUND;
        if !light && clear != MENU_BACKGROUND {
            return clear;
        }
        let Some(mood) = self.eased_mood() else {
            return clear;
        };
        let (top, bottom) = backdrop(mood, light);
        let (w, h) = (batch.width, batch.height);
        let mut back = HudBatch::new([1, 1]);
        (back.scale, back.width, back.height) = (batch.scale, w, h);
        if light {
            back.fill_gradient(
                &self.gui.atlas,
                (0.0, 0.0, w, h),
                packed(top),
                packed(bottom),
            );
        } else {
            let all = [0.0, 0.0, w, h];
            self.draw_panel_sky(&mut back, all, all, false);
            back.fill(&self.gui.atlas, 0.0, 0.0, w, h, rgba(0, 0, 0, 0x70));
        }
        prepend(batch, back);
        let [r, g, b] = bottom.map(f64::from);
        wgpu::Color { r, g, b, a: 1.0 }
    }
}

/// `under`'s vertices and blend runs (the sun's added light) drawn before everything in
/// `batch`: what was there moves along after them.
fn prepend(batch: &mut HudBatch, under: HudBatch) {
    let n = under.vertices.len() as u32;
    batch.vertices.splice(0..0, under.vertices);
    for (range, _) in &mut batch.blends {
        *range = range.start + n..range.end + n;
    }
    batch.blends.splice(0..0, under.blends);
    if batch.blur_at > 0 {
        batch.blur_at += n;
    }
    batch.patches.extend(under.patches);
}
