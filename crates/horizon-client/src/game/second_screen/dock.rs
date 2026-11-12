//! The keyboard dock. A text box edited on a vanilla-borrowed menu brings up the system
//! keyboard over the bottom of the lower screen; where it would cover what the player needs
//! (the creative search's results, the anvil's result and cost), the menu's GUI moves to the
//! window, beside the showcase, easing there, and back when the keyboard goes. Nothing else
//! on the window is covered or pushed off its sides: the window's caption keeps the bottom,
//! the showcase gives way sideways, and the tooltip (which would sit where the dock is) is
//! left out while docked. The lower screen shows nothing of the menu meanwhile, and a tap on
//! it puts the keyboard away.
//!
//! Which menus: those whose edited field sits in the top third of the lower screen but whose
//! working area does not (the creative search, the anvil's rename). The recipe book's search
//! is on the window already; the options, statistics and advancements screens keep every
//! field in their header. The moved-whole screens with low fields (the world options' port
//! box, the block editor) are not docked yet.
use super::*;

thread_local! {
    /// How far docked, 0 to 1, and when it was last eased.
    static DOCK: Cell<(f32, Option<Instant>)> = const { Cell::new((0.0, None)) };
}

/// Where the docked GUI goes on the window: its top-left corner and scale.
#[derive(Clone, Copy)]
pub(super) struct Docked {
    pub x: f32,
    pub y: f32,
    pub k: f32,
}

/// The caption and hint line along the window's bottom: the dock stays above it.
const CAPTION: f32 = 58.0;
/// Between the showcase and the dock, and the window's edges.
const GAP: f32 = 6.0;
const EDGE: f32 = 4.0;
/// The showcase's width.
const SHOWCASE: f32 = 96.0;

impl Game<'_> {
    /// The open menu has an edited text box and its GUI is where the keyboard covers it.
    pub(super) fn dock_wanted(&self) -> bool {
        if !crate::platform::active() {
            return false;
        }
        match self.screen {
            // The results grid fills the screen below the field.
            Screen::Creative => self.creative_screen.tab().search,
            Screen::Inventory => self.station_typing(),
            _ => false,
        }
    }

    /// How far docked now, eased about 0.1 s; call it as often as wanted.
    pub(super) fn dock_amount(&self) -> f32 {
        let now = Instant::now();
        let target = if self.dock_wanted() { 1.0 } else { 0.0 };
        let (at, last) = DOCK.get();
        let dt = last
            .map_or(1.0, |t| now.duration_since(t).as_secs_f32())
            .min(0.25);
        let mut at = at + (target - at) * (1.0 - (-18.0 * dt).exp());
        if (target - at).abs() < 0.01 {
            at = target;
        }
        DOCK.set((at, Some(now)));
        at
    }

    /// Where the showcase goes, and the dock, for a window of `batch`'s size: the pair is
    /// centred, the GUI at the largest of full, three-quarters and half size that fits beside
    /// the showcase and above the caption. Eased between the showcase's own place and this.
    pub(super) fn dock_place(
        &self,
        batch: &HudBatch,
        center: [f32; 2],
    ) -> ([f32; 2], Option<Docked>) {
        let amount = self.dock_amount();
        if amount < 0.01 {
            return (center, None);
        }
        let Some([_, _, gw, gh]) = self.container_region(batch) else {
            return (center, None);
        };
        let (w, h) = (batch.width, batch.height);
        let room = h - CAPTION - 2.0 * EDGE;
        let k = [1.0, 0.75, 0.5]
            .into_iter()
            .find(|k| gw * k + GAP + SHOWCASE <= w - 2.0 * EDGE && gh * k <= room)
            .unwrap_or(0.5);
        let pair = gw * k + GAP + SHOWCASE;
        let x0 = ((w - pair) / 2.0).max(EDGE).floor();
        let showcase_x = x0 + SHOWCASE / 2.0;
        let x = x0 + SHOWCASE + GAP;
        let y = (h * 0.40 - gh * k / 2.0).clamp(EDGE, (h - CAPTION - gh * k).max(EDGE));
        // Smooth, not linear: slow at both ends. The GUI rises from below the window.
        let t = amount * amount * (3.0 - 2.0 * amount);
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        (
            [lerp(center[0], showcase_x).floor(), center[1]],
            Some(Docked {
                x: x.floor(),
                y: lerp(h, y).floor(),
                k,
            }),
        )
    }

    /// The menu's GUI again, on the window at `dock`: drawn as the lower screen draws it (the
    /// pointer away, no dimming, no side effects) and cut to its art, scaled and moved.
    pub(super) fn draw_docked(&mut self, batch: &mut HudBatch, dock: Docked) {
        let Some([rx, ry, rw, rh]) = self.container_region(batch) else {
            return;
        };
        // What the copy registers (fields, buttons) is the lower screen's business.
        let saved = horizon_client::widgets::take();
        let start = batch.vertices.len();
        let cursor = std::mem::replace(&mut self.cursor, (-10_000.0, -10_000.0));
        super::LOWER_PASS.set(true);
        if self.screen == Screen::Creative {
            self.draw_creative(batch);
        } else {
            self.draw_inventory(batch);
        }
        super::LOWER_PASS.set(false);
        self.cursor = cursor;
        horizon_client::widgets::put(saved);
        batch.clip_since(start, rx, ry, rx + rw, ry + rh);
        hud::transform_since(batch, start, [rx, ry], dock.k, [dock.x - rx, dock.y - ry]);
    }
}
