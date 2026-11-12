//! Containers on the dual screen, one layout for all of them (chests, barrels, shulker boxes,
//! hoppers, dispensers, furnaces, brewing stands, enchanting tables, anvils and the other
//! workstations): on the lower screen vanilla's whole screen (its art from the resource packs,
//! its slots, the inventory and hotbar, progress and buttons, drawn and clicked as vanilla's),
//! as large as it fits in whole pixels; on the window, over the world, the container's block
//! large, its title, the tapped item's name and the controls. Screens laid out otherwise
//! (trading, the beacon), whose buttons the controller's menu pointer works, are moved whole
//! instead (`second_screen_menu`).
use super::menu::inside;
use super::*;
use horizon_client::{item_tooltip, menu::Input};
use std::collections::BTreeMap;

/// Where things go on the lower screen.
struct Layout {
    scale: f32,
    /// Vanilla's screen, in the window's GUI pixels.
    region: [f32; 4],
    /// From the window's GUI pixels to the lower screen's.
    offset: [f32; 2],
}

#[derive(Default)]
struct State {
    /// The slot last tapped or chosen, its name on the window; where the carried stack is drawn.
    focus: Option<usize>,
    at: [f32; 2],
    /// The controller's row and column (`pad_rows`), and whether it was used last.
    pad: (usize, usize),
    pad_active: bool,
    /// A finger came down on vanilla's screen off its slots: it holds the window's mouse
    /// button down there (a button, a scrolled list, a field) until it lifts.
    pressing: bool,
}

thread_local! {
    static CONTAINER: RefCell<State> = RefCell::new(State::default());
}

/// Kept around vanilla's screen on the lower screen, in its GUI pixels.
const MARGIN: f32 = 2.0;

/// The block a menu type stands for, as the window's icon.
fn container_icon(menu: &str) -> &'static str {
    match menu.trim_start_matches("minecraft:") {
        m if m.starts_with("generic_9x") => "minecraft:chest",
        "generic_3x3" => "minecraft:dispenser",
        "crafter_3x3" => "minecraft:crafter",
        "furnace" => "minecraft:furnace",
        "blast_furnace" => "minecraft:blast_furnace",
        "smoker" => "minecraft:smoker",
        "brewing_stand" => "minecraft:brewing_stand",
        "enchantment" => "minecraft:enchanting_table",
        "anvil" => "minecraft:anvil",
        "beacon" => "minecraft:beacon",
        "hopper" => "minecraft:hopper",
        "merchant" => "minecraft:emerald",
        "shulker_box" => "minecraft:shulker_box",
        "smithing" => "minecraft:smithing_table",
        "loom" => "minecraft:loom",
        "stonecutter" => "minecraft:stonecutter",
        "grindstone" => "minecraft:grindstone",
        "cartography_table" => "minecraft:cartography_table",
        _ => "minecraft:chest",
    }
}

impl Game<'_> {
    /// The beacon's or a merchant's screen is open: moved whole (`second_screen_menu`).
    pub(super) fn container_moved_whole(&self) -> bool {
        let beacon = self
            .station
            .as_ref()
            .is_some_and(|s| s.kind == super::super::stations::Kind::Beacon);
        beacon
            || matches!(
                self.remote_menu().1.as_str(),
                "minecraft:merchant" | "minecraft:beacon"
            )
    }

    /// The open container's whole screen, down to the border under the hotbar, in the
    /// window's GUI pixels; None for the screens moved whole.
    pub(super) fn container_region(&self, window: &HudBatch) -> Option<[f32; 4]> {
        if self.container_moved_whole() {
            return None;
        }
        let width = self.native_screen_width().unwrap_or(176.0);
        let (ox, oy) = self.inventory_origin(window);
        // Vanilla's hotbar row is 24 above the bottom of its art (25 for the chests'), a mod
        // screen gives its height.
        let height = self
            .native_screen_height()
            .unwrap_or(self.slot_position(0).1 + 25.0);
        Some([ox, oy, width, height])
    }

    /// The largest whole GUI scale at which vanilla's screen fits, centred.
    fn container_layout(size: [u32; 2], region: [f32; 4]) -> Layout {
        let [rx, ry, rw, rh] = region;
        let need = [rw + 2.0 * MARGIN, rh + 2.0 * MARGIN];
        let fit = (size[0] as f32 / need[0]).min(size[1] as f32 / need[1]);
        let scale = fit.floor().max(1.0);
        let (w, h) = (
            (size[0] as f32 / scale).ceil(),
            (size[1] as f32 / scale).ceil(),
        );
        Layout {
            scale,
            region,
            offset: [((w - rw) / 2.0).floor() - rx, ((h - rh) / 2.0).floor() - ry],
        }
    }

    /// The window's GUI batch and the lower screen's layout, when the open screen is one.
    fn container_view(&self, size: [u32; 2]) -> Option<Layout> {
        let window = self
            .renderer
            .as_ref()
            .map(|r| HudBatch::new([r.config.width, r.config.height]))?;
        let region = self.container_region(&window)?;
        Some(Self::container_layout(size, region))
    }

    /// One frame of the lower screen with a container open; false for the screens moved whole.
    pub(super) fn container_frame(
        &mut self,
        events: &dyn EventLoopCtl,
        id: WindowId,
        size: [u32; 2],
        touches: &[crate::second_screen::Touch],
    ) -> bool {
        use crate::second_screen::Touch;
        let Some(layout) = self.container_view(size) else {
            return false;
        };
        let mut batch = HudBatch::new(size);
        batch.scale = layout.scale;
        batch.width = (size[0] as f32 / layout.scale).ceil();
        batch.height = (size[1] as f32 / layout.scale).ceil();
        let [rx, ry, rw, rh] = layout.region;
        let lower_region = [rx + layout.offset[0], ry + layout.offset[1], rw, rh];
        let window = self
            .renderer
            .as_ref()
            .map(|r| HudBatch::new([r.config.width, r.config.height]));
        let to_window = |p: [f32; 2]| [p[0] - layout.offset[0], p[1] - layout.offset[1]];
        // The slot at `p` (the container's, the inventory's or the hotbar's), found where it
        // is on the window.
        let slot_at = |game: &Self, p: [f32; 2]| {
            let window = window.as_ref().filter(|_| inside(lower_region, p))?;
            let [x, y] = to_window(p);
            game.slot_at(window, x, y)
        };
        let click = |game: &mut Self, slot: usize| game.container_click(slot, 0, Input::Pickup);
        let pressing = || CONTAINER.with_borrow(|s| s.pressing);
        let set_pressing = |on: bool| CONTAINER.with_borrow_mut(|s| s.pressing = on);
        for touch in touches {
            match *touch {
                Touch::Down([x, y]) => {
                    let p = [x / layout.scale, y / layout.scale];
                    CONTAINER.with_borrow_mut(|s| (s.at, s.pad_active) = (p, false));
                    match slot_at(self, p) {
                        Some(slot) => self.slot_down(Some(slot), p, click),
                        // Vanilla's buttons, lists and fields: pressed where they are on the
                        // window, held while the finger is down.
                        None if inside(lower_region, p) => {
                            set_pressing(true);
                            let state = Some(ElementState::Pressed);
                            self.window_mouse(events, id, to_window(p), state);
                        }
                        None => self.slot_down(None, p, click),
                    }
                }
                Touch::Move([x, y]) => {
                    let p = [x / layout.scale, y / layout.scale];
                    CONTAINER.with_borrow_mut(|s| s.at = p);
                    if pressing() {
                        self.window_mouse(events, id, to_window(p), None);
                    } else {
                        Self::slot_move(p);
                    }
                }
                Touch::Up([x, y]) => {
                    let p = [x / layout.scale, y / layout.scale];
                    if pressing() {
                        set_pressing(false);
                        let state = Some(ElementState::Released);
                        self.window_mouse(events, id, to_window(p), state);
                    } else {
                        let slot = slot_at(self, p);
                        self.slot_up(slot, click);
                    }
                }
            }
            if !self.screen_on_second_screen() || self.screen != Screen::Inventory {
                return true;
            }
        }
        let right = |game: &mut Self, slot: usize| game.container_click(slot, 1, Input::Pickup);
        self.slot_hold(click, right);
        // Vanilla's screen, at the window's size with the pointer away (no hover, no tooltip,
        // no carried stack), cut to its art (off the window's dimmed background) and moved
        // here.
        let start = batch.vertices.len();
        let full = [batch.width, batch.height];
        if let Some(window) = self
            .renderer
            .as_ref()
            .map(|r| HudBatch::new([r.config.width, r.config.height]))
        {
            [batch.width, batch.height] = [window.width, window.height];
        }
        let cursor = std::mem::replace(&mut self.cursor, (-10_000.0, -10_000.0));
        self.draw_inventory(&mut batch);
        self.cursor = cursor;
        [batch.width, batch.height] = full;
        batch.clip_since(start, rx, ry, rx + rw, ry + rh);
        hud::transform_since(&mut batch, start, [0.0, 0.0], 1.0, layout.offset);
        // The controller's slot, and the carried stack on it or where the last tap was.
        let (active, focus) = CONTAINER.with_borrow(|s| (s.pad_active, s.pad));
        if active && let Some(rect) = self.container_slot_rect(&layout, focus) {
            super::menu::highlight(&mut batch, &self.gui.atlas, rect);
            let at = [rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0];
            CONTAINER.with_borrow_mut(|s| s.at = at);
        }
        if let Some(stack) = self.carried.clone() {
            let [x, y] = CONTAINER.with_borrow(|s| s.at);
            self.draw_item(&mut batch, &stack, x - 8.0, y - 8.0);
        }
        self.present_lower(&mut batch, MENU_BACKGROUND);
        true
    }

    fn container_click(&mut self, slot: usize, button: i32, input: Input) {
        CONTAINER.with_borrow_mut(|s| s.focus = Some(slot));
        self.menu_click(Some(slot), button, input);
    }

    /// The slots as a controller moves over them: vanilla's screen's rows, top to bottom (the
    /// container's, the inventory's, the hotbar), each left to right.
    fn container_pad_rows(&self) -> Vec<Vec<usize>> {
        let mut rows: BTreeMap<i32, Vec<(i32, usize)>> = BTreeMap::new();
        for slot in self.slots() {
            let (x, y) = self.slot_position(slot);
            rows.entry(y.round() as i32)
                .or_default()
                .push((x.round() as i32, slot));
        }
        rows.into_values()
            .map(|mut row| {
                row.sort();
                row.into_iter().map(|(_, s)| s).collect()
            })
            .collect()
    }

    /// Where a slot of `pad_rows` is on the lower screen.
    fn container_slot_rect(&self, layout: &Layout, (row, col): (usize, usize)) -> Option<[f32; 4]> {
        let rows = self.container_pad_rows();
        let slot = *rows.get(row)?.get(col)?;
        let (sx, sy) = self.slot_position(slot);
        let window = self
            .renderer
            .as_ref()
            .map(|r| HudBatch::new([r.config.width, r.config.height]))?;
        let (ox, oy) = self.inventory_origin(&window);
        Some([
            ox + sx - 1.0 + layout.offset[0],
            oy + sy - 1.0 + layout.offset[1],
            18.0,
            18.0,
        ])
    }

    /// A controller button with a container open; true when taken: the pad moves over the
    /// slots, A takes or puts down all, Y half, X moves the stack across (B exits, as ever).
    pub(super) fn container_pad(&mut self, button: horizon_client::gamepad::Button) -> bool {
        use horizon_client::gamepad::Button as B;
        let Some(window) = self
            .renderer
            .as_ref()
            .map(|r| HudBatch::new([r.config.width, r.config.height]))
        else {
            return false;
        };
        if self.container_region(&window).is_none() {
            return false;
        }
        if super::menu::hotbar_shoulder(self, button) {
            return true;
        }
        let rows = self.container_pad_rows();
        let (row, col) = CONTAINER.with_borrow(|s| s.pad);
        let row = row.min(rows.len() - 1);
        let moved = match button {
            B::DPadUp => Some((row.saturating_sub(1), col)),
            B::DPadDown => Some(((row + 1).min(rows.len() - 1), col)),
            B::DPadLeft => Some((row, col.saturating_sub(1))),
            B::DPadRight => Some((row, col + 1)),
            _ => None,
        };
        if let Some((row, col)) = moved {
            // The same place across, as near as the new row has.
            let col = col.min(rows[row].len().saturating_sub(1));
            CONTAINER.with_borrow_mut(|s| (s.pad, s.pad_active) = ((row, col), true));
            return true;
        }
        let Some(&slot) = rows[row].get(col.min(rows[row].len().saturating_sub(1))) else {
            return false;
        };
        let (button, input) = match button {
            B::South => (0, Input::Pickup),
            B::North => (1, Input::Pickup),
            B::West => (0, Input::QuickMove),
            _ => return false,
        };
        CONTAINER.with_borrow_mut(|s| s.pad_active = true);
        self.container_click(slot, button, input);
        true
    }

    /// The window while a container is open: its block large (spun by a finger on the window),
    /// and along the bottom the tapped item's name under the container's title (or the title
    /// alone) and the controls.
    pub(super) fn draw_container_upper(&mut self, batch: &mut HudBatch) {
        let menu = self.remote_menu().1;
        let title = self
            .remote_menu_title()
            .map(|t| t.plain())
            .unwrap_or_else(|| self.translate("container.inventory", "Inventory"));
        let (w, h) = (batch.width, batch.height);
        // The item held, else the slot the controller is on or the last one tapped.
        let (focus, pad_active, pad) = CONTAINER.with_borrow(|s| (s.focus, s.pad_active, s.pad));
        let focus = if pad_active {
            let rows = self.container_pad_rows();
            rows.get(pad.0)
                .and_then(|row| row.get(pad.1.min(row.len().saturating_sub(1))))
                .copied()
                .or(focus)
        } else {
            focus
        };
        let stack = self.carried.clone().or_else(|| match focus {
            Some(slot @ 0..36) => self.inventory.get(slot).cloned().flatten(),
            Some(slot) => self.slot_view(slot),
            None => None,
        });
        // An open recipe book on the left, where vanilla puts it: the rest moves over beside
        // it, easing there.
        let book = self.draw_open_recipe_book(batch);
        let left = book_slide(book.map(|[x, _, bw, _]| x + bw + 4.0));
        let span = [left, w - left];
        let center = [(left + span[1] / 2.0).floor(), (h * 0.40).floor()];
        let shown = stack
            .clone()
            .unwrap_or_else(|| ItemStack::new(container_icon(&menu), 1));
        self.draw_showcase(batch, &shown, center);
        let name =
            stack.map(|s| item_tooltip::name_line(&s, &|key| self.pack.names.display_get(key)));
        let (heading, name) = match &name {
            Some(name) => (Some(title.as_str()), name.as_str()),
            None => (None, title.as_str()),
        };
        let hint = self.pad_hint(
            "{south} Take All    {north} Take Half    {west} Quick Move    {lb}/{rb} Hotbar    {east} Exit",
        );
        self.draw_upper_caption_in(batch, span, heading, Some(name), &hint);
    }
}

thread_local! {
    /// Where the window's showcase and caption start from the left, eased, and when.
    static SLIDE: Cell<(f32, Option<std::time::Instant>)> = const { Cell::new((0.0, None)) };
}

/// The left edge the showcase and caption ease toward: `to` (beside an open recipe book) or
/// the window's own edge; about 0.15 s to get there.
fn book_slide(to: Option<f32>) -> f32 {
    let now = std::time::Instant::now();
    let target = to.unwrap_or(0.0);
    let (at, last) = SLIDE.get();
    let dt = last
        .map_or(1.0, |t| now.duration_since(t).as_secs_f32())
        .min(0.25);
    let mut at = at + (target - at) * (1.0 - (-18.0 * dt).exp());
    if (target - at).abs() < 0.5 {
        at = target;
    }
    SLIDE.set((at, Some(now)));
    at.floor()
}
