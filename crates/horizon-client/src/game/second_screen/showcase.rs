//! The item shown large on the window while the creative screen or a container is open on the
//! second screen: it floats, a finger on the window spins it in 3D (it keeps turning after the
//! finger lifts and slows to a stop), and a tap makes it bounce. Under it, along the bottom,
//! the item's name and the controls.
use super::menu::centered;
use super::*;
use glam::{Mat4, Vec3};
use std::time::{Duration, Instant};

/// Radians a finger turns the item per window pixel: a swipe across the window turns it 1.5
/// times.
const TURN_PER_WIDTH: f32 = 3.0 * std::f32::consts::PI;
/// How quickly a free spin slows (per second): it loses half its speed in about 0.4 s.
const SPIN_FRICTION: f32 = 1.7;
/// The tilt's spring back to upright, and its damping: a little overshoot, then rest.
const TILT_STIFFNESS: f32 = 14.0;
const TILT_DAMPING: f32 = 4.5;
/// The fastest spin a flick gives, in radians per second.
const MAX_SPIN: f32 = 40.0;
/// A finger held still this long before lifting lets go of the item without a spin.
const HELD_STILL: Duration = Duration::from_millis(90);
/// Down and up within this time, and within `TAP_SLOP` window pixels, is a tap.
const TAP_TIME: Duration = Duration::from_millis(300);
const TAP_SLOP: f32 = 12.0;
/// How long a tap's bounce lasts.
const BOUNCE: f32 = 0.9;
/// The item's size: 5.5 times a slot's.
const SCALE: f32 = 5.5;
/// The player's size, in GUI pixels per block.
const DOLL_SIZE: f32 = 50.0;
/// How long the player's turn and look at a new armor piece last, and its arm's swing.
const ARMOR_REACTION: f32 = 1.5;
const SWING: f32 = 0.3;

struct Finger {
    id: u64,
    down: [f32; 2],
    at: [f32; 2],
    since: Instant,
    moved_at: Instant,
    moved: bool,
}

struct Spin {
    yaw: f32,
    pitch: f32,
    /// Radians per second.
    yaw_speed: f32,
    pitch_speed: f32,
    finger: Option<Finger>,
    bounce: Option<Instant>,
    /// The item last shown: another one starts facing the front, still.
    item: Option<String>,
    /// When the item was last drawn: touches spin it only while it is shown.
    drawn: Option<Instant>,
    last: Option<Instant>,
}

/// What the player wore and held when last drawn, and the reactions under way.
#[derive(Default)]
struct Doll {
    armor: [Option<std::sync::Arc<str>>; 4],
    held: Option<std::sync::Arc<str>>,
    seen: Option<Instant>,
    /// A new piece put on: when, and its slot (head to feet).
    armor_at: Option<(Instant, usize)>,
    swing_at: Option<Instant>,
}

thread_local! {
    /// The finger on the window's recipe book, pressing the window's mouse there.
    static BOOK_FINGER: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
    static DOLL: RefCell<Doll> = RefCell::new(Doll::default());
    static SPIN: RefCell<Spin> = const {
        RefCell::new(Spin {
            yaw: 0.0,
            pitch: 0.0,
            yaw_speed: 0.0,
            pitch_speed: 0.0,
            finger: None,
            bounce: None,
            item: None,
            drawn: None,
            last: None,
        })
    };
}

/// The nearest angle to 0 that is a whole number of turns from `a`.
fn wrap(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (a + PI).rem_euclid(TAU) - PI
}

impl Spin {
    /// The spin and tilt `dt` seconds on, with no finger holding the item.
    fn step(&mut self, dt: f32) {
        if self.finger.is_some() {
            return;
        }
        self.yaw = wrap(self.yaw + self.yaw_speed * dt);
        self.yaw_speed *= (-SPIN_FRICTION * dt).exp();
        if self.yaw_speed.abs() < 0.01 {
            self.yaw_speed = 0.0;
        }
        // The tilt springs back upright the shorter way round.
        self.pitch = wrap(self.pitch);
        self.pitch_speed += -self.pitch * TILT_STIFFNESS * dt;
        self.pitch_speed *= (-TILT_DAMPING * dt).exp();
        self.pitch += self.pitch_speed * dt;
    }

    /// The bounce's lift (GUI pixels up) and scale, `t` seconds after the tap: hops that die
    /// away, the item swelling a little at each.
    fn bounce(t: f32) -> (f32, f32) {
        if !(0.0..BOUNCE).contains(&t) {
            return (0.0, 1.0);
        }
        let hop = (t * std::f32::consts::PI / 0.28).sin().abs();
        let decay = (-5.5 * t).exp();
        (12.0 * decay * hop, 1.0 + 0.14 * decay * (t * 18.0).sin())
    }
}

/// One frame of the spin of the thing shown, `key` (another starts facing the front, still):
/// its turn, tilt, bounce lift (GUI pixels up) and scale.
fn spin_frame(key: &str) -> (f32, f32, f32, f32) {
    let now = Instant::now();
    SPIN.with_borrow_mut(|s| {
        if s.item.as_deref() != Some(key) {
            (s.yaw, s.pitch, s.yaw_speed, s.pitch_speed) = (0.0, 0.0, 0.0, 0.0);
            s.bounce = None;
            s.item = Some(key.to_owned());
        }
        // A screen opened again starts from where it was, not from a long step.
        let dt = s
            .last
            .map_or(0.0, |l| now.duration_since(l).as_secs_f32())
            .min(0.05);
        s.step(dt);
        (s.last, s.drawn) = (Some(now), Some(now));
        let (lift, scale) = s.bounce.map_or((0.0, 1.0), |b| {
            Spin::bounce(now.duration_since(b).as_secs_f32())
        });
        (s.yaw, s.pitch, lift, scale)
    })
}

impl Game<'_> {
    /// A touch on the window while a screen is on the second screen. That screen is touched
    /// there, so the window's touch controls stay hidden and the input stays what it was: the
    /// touch is taken here, spinning the item shown when there is one, or (on an open recipe
    /// book, which the window draws) moving and pressing the window's mouse there. False
    /// otherwise.
    pub(in crate::game) fn upper_touch(
        &mut self,
        t: &winit::event::Touch,
        events: &dyn EventLoopCtl,
        id: WindowId,
    ) -> bool {
        use winit::event::TouchPhase;
        if !self.menu_on_lower() {
            return false;
        }
        if self.upper_book_touch(t, events, id) {
            return true;
        }
        let now = Instant::now();
        let p = [t.location.x as f32, t.location.y as f32];
        let width = self.renderer.as_ref().map_or(1, |r| r.config.width).max(1) as f32;
        let tapped = SPIN.with_borrow_mut(|s| {
            let shown = s
                .drawn
                .is_some_and(|d| now.duration_since(d) < Duration::from_millis(250));
            if !shown {
                s.finger = None;
                return false;
            }
            let ours = s.finger.as_ref().is_some_and(|f| f.id == t.id);
            match t.phase {
                // Caught: a spinning item stops under the finger.
                TouchPhase::Started if s.finger.is_none() => {
                    s.finger = Some(Finger {
                        id: t.id,
                        down: p,
                        at: p,
                        since: now,
                        moved_at: now,
                        moved: false,
                    });
                    (s.yaw_speed, s.pitch_speed) = (0.0, 0.0);
                    false
                }
                TouchPhase::Moved if ours => {
                    let k = TURN_PER_WIDTH / width;
                    let f = s.finger.as_mut().expect("ours");
                    let (dx, dy) = (p[0] - f.at[0], p[1] - f.at[1]);
                    let dt = now
                        .duration_since(f.moved_at)
                        .as_secs_f32()
                        .max(1.0 / 240.0);
                    f.moved |= (p[0] - f.down[0]).hypot(p[1] - f.down[1]) > TAP_SLOP;
                    (f.at, f.moved_at) = (p, now);
                    s.yaw += dx * k;
                    s.pitch += dy * k;
                    // The finger's speed, smoothed over the last few moves: the flick's spin.
                    let blend =
                        |v: f32, d: f32| (v + (d * k / dt - v) * 0.5).clamp(-MAX_SPIN, MAX_SPIN);
                    s.yaw_speed = blend(s.yaw_speed, dx);
                    s.pitch_speed = blend(s.pitch_speed, dy);
                    false
                }
                TouchPhase::Ended | TouchPhase::Cancelled if ours => {
                    let f = s.finger.take().expect("ours");
                    if now.duration_since(f.moved_at) > HELD_STILL {
                        (s.yaw_speed, s.pitch_speed) = (0.0, 0.0);
                    }
                    let tap = t.phase == TouchPhase::Ended
                        && !f.moved
                        && now.duration_since(f.since) < TAP_TIME;
                    if tap {
                        s.bounce = Some(now);
                    }
                    tap
                }
                _ => false,
            }
        });
        if tapped {
            self.android_touch_haptic(10);
        }
        true
    }

    /// A finger on the window's open recipe book: the window's mouse, pressed where it comes
    /// down, following it, let go where it lifts, as on a desktop's book. True when taken.
    fn upper_book_touch(
        &mut self,
        t: &winit::event::Touch,
        events: &dyn EventLoopCtl,
        id: WindowId,
    ) -> bool {
        use winit::event::TouchPhase;
        let Some(window) = self
            .renderer
            .as_ref()
            .map(|r| HudBatch::new([r.config.width, r.config.height]))
        else {
            return false;
        };
        let gui = [
            t.location.x as f32 / window.scale,
            t.location.y as f32 / window.scale,
        ];
        let ours = BOOK_FINGER.get() == Some(t.id);
        match t.phase {
            TouchPhase::Started => {
                // Only where the window draws it: vanilla's screens moved down (a furnace's),
                // not the inventory's and crafting's own layouts.
                let vanilla =
                    !self.crafting_screen_open() && self.container_region(&window).is_some();
                let on_book = vanilla
                    && self
                        .open_recipe_book_rect(&window)
                        .is_some_and(|r| super::menu::inside(r, gui));
                if !on_book || BOOK_FINGER.get().is_some() {
                    return false;
                }
                BOOK_FINGER.set(Some(t.id));
                self.android_touch_haptic(8);
                self.window_mouse(events, id, gui, Some(ElementState::Pressed));
            }
            TouchPhase::Moved if ours => self.window_mouse(events, id, gui, None),
            TouchPhase::Ended | TouchPhase::Cancelled if ours => {
                BOOK_FINGER.set(None);
                self.window_mouse(events, id, gui, Some(ElementState::Released));
            }
            _ => return false,
        }
        true
    }

    /// A screen is open on the second screen (not at the title, whose window keeps its
    /// logo): the window shows what goes with it and takes no touches of its own.
    pub(in crate::game) fn menu_on_lower(&self) -> bool {
        active() && self.title.is_none() && self.screen_on_second_screen()
    }

    /// `stack` large around `center` (GUI pixels): floating, turned as the finger left it,
    /// bouncing after a tap.
    pub(super) fn draw_showcase(&self, batch: &mut HudBatch, stack: &ItemStack, center: [f32; 2]) {
        let (yaw, pitch, lift, scale) = spin_frame(&stack.item);
        // The float: a slow bob and a slight sway.
        let t = self.started.elapsed().as_secs_f32();
        let bob = (t * 1.9).sin() * 2.5;
        let sway = (t * 1.3).sin() * 0.06;
        let size = 16.0 * SCALE * scale;
        let pose = Mat4::from_translation(Vec3::new(center[0], center[1] - bob - lift, 0.0))
            * Mat4::from_scale(Vec3::new(size, -size, size))
            * Mat4::from_rotation_z(sway)
            * Mat4::from_rotation_x(pitch)
            * Mat4::from_rotation_y(yaw);
        let one = ItemStack {
            count: 1,
            ..stack.clone()
        };
        self.draw_item_posed(batch, &one, pose);
    }

    /// The player standing around `center` (GUI pixels), holding what the hotbar holds: a
    /// swipe spins it and a tap makes it hop, as an item; it sways a little while idle, turns
    /// round once and looks at a new armor piece when one goes on, and swings its arm when what
    /// it holds changes.
    pub(super) fn draw_showcase_player(&self, batch: &mut HudBatch, center: [f32; 2]) {
        use std::f32::consts::{PI, TAU};
        let now = Instant::now();
        let (spin, _, hop, _) = spin_frame("minecraft:player");
        let armor = std::array::from_fn(|i| self.armor[i].as_ref().map(|s| s.item.clone()));
        let held = self
            .inventory
            .get(self.selected)
            .and_then(Option::as_ref)
            .map(|s| s.item.clone());
        let (armor_at, swing_at) = DOLL.with_borrow_mut(|d| {
            // Just opened: what it wears and holds already is no news.
            let fresh = d
                .seen
                .is_none_or(|t| now.duration_since(t) > Duration::from_millis(250));
            if !fresh {
                // The lowest slot changed wins: the feet are looked at longest.
                for (slot, (new, old)) in armor.iter().zip(&d.armor).enumerate() {
                    if new.is_some() && new != old {
                        d.armor_at = Some((now, slot));
                    }
                }
                if held != d.held {
                    d.swing_at = Some(now);
                }
            }
            (d.armor, d.held, d.seen) = (armor, held, Some(now));
            let since = |t: Instant| now.duration_since(t).as_secs_f32();
            d.armor_at = d.armor_at.filter(|&(t, _)| since(t) < ARMOR_REACTION);
            d.swing_at = d.swing_at.filter(|&t| since(t) < SWING);
            (
                d.armor_at.map(|(t, slot)| (since(t), slot)),
                d.swing_at.map(since),
            )
        });
        let t = self.started.elapsed().as_secs_f32();
        let mut yaw = (t * 0.8).sin() * 12.0 - spin.to_degrees();
        let (mut lift, mut look) = (hop / DOLL_SIZE, 0.0);
        if let Some((t, slot)) = armor_at {
            // A hop, a full turn easing out, then a look at the piece: up a little at the
            // helmet, further down from the chestplate to the boots.
            lift += 0.3 * (PI * (t / 0.35).min(1.0)).sin();
            let turn = 1.0 - (1.0 - (t / 0.8).min(1.0)).powi(3);
            yaw -= (TAU * turn).to_degrees();
            let gaze = [-12.0, 28.0, 42.0, 55.0][slot];
            look = gaze * (PI * ((t - 0.5) / (ARMOR_REACTION - 0.5)).clamp(0.0, 1.0)).sin();
        }
        let pose = crate::game::perspective::DollPose {
            lift,
            head_pitch: look,
            swing: swing_at.map_or(0.0, |t| t / SWING),
            hands: true,
        };
        // Room around the player for its turn, its hop and what it holds.
        let (hw, hh) = (DOLL_SIZE * 1.6, DOLL_SIZE * 1.5);
        let rect = [
            center[0] - hw,
            center[1] - hh,
            center[0] + hw,
            center[1] + hh,
        ];
        self.draw_player_posed(batch, rect, DOLL_SIZE, 0.0625, Some(yaw), &pose);
    }

    /// Along the window's bottom edge: `heading` small, `name` at twice the text size under
    /// it, and the controls (`hint`, in the controller's glyphs) at the very bottom.
    pub(super) fn draw_upper_caption(
        &self,
        batch: &mut HudBatch,
        heading: Option<&str>,
        name: Option<&str>,
        hint: &str,
    ) {
        let span = [0.0, batch.width];
        self.draw_upper_caption_in(batch, span, heading, name, hint);
    }

    /// [`Self::draw_upper_caption`] with the heading and name centred across `[x, width]` of the
    /// window (beside a recipe book); the controls stay centred on the whole window.
    pub(super) fn draw_upper_caption_in(
        &self,
        batch: &mut HudBatch,
        [x0, w]: [f32; 2],
        heading: Option<&str>,
        name: Option<&str>,
        hint: &str,
    ) {
        let atlas = &self.gui.atlas;
        let h = batch.height;
        let hint_y = (h - 14.0).floor();
        // The controls stay put across the whole window; only the name moves aside for a book.
        centered(batch, atlas, hint, [0.0, hint_y, batch.width], 0xFFCC_CCCC, true);
        let name_y = hint_y - 26.0;
        if let Some(name) = name {
            let tw = HudBatch::text_width(atlas, name).max(1.0);
            let k = (2.0f32).min((w - 16.0) / tw).max(1.0);
            let x = (x0 + (w - tw * k) / 2.0).floor();
            let start = batch.vertices.len();
            batch.text(atlas, name, x, name_y, u32::MAX, true);
            hud::transform_since(batch, start, [x, name_y], k, [0.0, 0.0]);
        }
        if let Some(heading) = heading {
            centered(
                batch,
                atlas,
                heading,
                [x0, name_y - 13.0, w],
                0xFFAA_AAAA,
                true,
            );
        }
    }
}
