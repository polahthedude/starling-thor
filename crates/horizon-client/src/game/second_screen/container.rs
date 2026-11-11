//! Containers on the dual screen, one layout for all of them (chests, barrels, shulker boxes,
//! hoppers, dispensers, furnaces, brewing stands, enchanting tables, anvils and the other
//! workstations), after New 3DS Edition's: on the lower screen the hotbar along the top, the
//! container's own part of vanilla's screen under it (its art, slots, progress and buttons,
//! drawn and clicked as vanilla's), then the inventory's 27 slots; on the window, over the
//! world, the container's block large, its title, the tapped item's name and the controls.
//! Vanilla's part keeps its art rather than the 3DS's simplified one: the larger screen has the
//! room. Screens laid out otherwise (trading, the beacon) are moved whole instead
//! (`second_screen_menu`).
use super::menu::{centered, inside, slot_frame};
use super::*;
use horizon_client::{item_tooltip, menu::Input};
use std::collections::BTreeMap;

/// Where things go on the lower screen.
struct Layout {
    scale: f32,
    /// The container's part of vanilla's screen, in the window's GUI pixels.
    region: [f32; 4],
    /// From the window's GUI pixels to the lower screen's.
    offset: [f32; 2],
    /// The inventory grid's top left on the lower screen.
    inventory: [f32; 2],
}

#[derive(Default)]
struct State {
    /// The slot last tapped or chosen, its name on the window; where the carried stack is drawn.
    focus: Option<usize>,
    at: [f32; 2],
    /// The controller's row and column (`pad_rows`), and whether it was used last.
    pad: (usize, usize),
    pad_active: bool,
}

thread_local! {
    static CONTAINER: RefCell<State> = RefCell::new(State::default());
}

const BAND: f32 = 24.0;

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
    /// The open container's part of vanilla's screen (above the inventory's label), in the
    /// window's GUI pixels; None for the screens moved whole.
    pub(super) fn container_region(&self, window: &HudBatch) -> Option<[f32; 4]> {
        let menu = self.remote_menu().1;
        let beacon = self
            .station
            .as_ref()
            .is_some_and(|s| s.kind == super::super::stations::Kind::Beacon);
        if beacon || matches!(menu.as_str(), "minecraft:merchant" | "minecraft:beacon") {
            return None;
        }
        let width = self.native_screen_width().unwrap_or(176.0);
        let (ox, oy) = self.inventory_origin(window);
        let top = self.slot_position(9).1 - 12.0;
        Some([ox, oy, width, top.max(20.0)])
    }

    /// The largest GUI scale up to the panel's at which the hotbar, the container and the
    /// inventory fit; what space is left goes evenly above and below them.
    fn container_layout(size: [u32; 2], region: [f32; 4]) -> Layout {
        let [rx, ry, rw, rh] = region;
        let need = [rw.max(162.0) + 8.0, BAND + 4.0 + rh + 6.0 + 54.0 + 4.0];
        let mut scale = touch::dual_panel_scale(size[0]);
        while scale > 1.0 && (size[0] as f32 / scale < need[0] || size[1] as f32 / scale < need[1])
        {
            scale -= 1.0;
        }
        let (w, h) = (
            (size[0] as f32 / scale).ceil(),
            (size[1] as f32 / scale).ceil(),
        );
        let spare = ((h - need[1]) / 2.0).max(0.0).floor();
        let top = BAND + 4.0 + spare;
        Layout {
            scale,
            region,
            offset: [((w - rw) / 2.0).floor() - rx, top - ry],
            inventory: [((w - 162.0) / 2.0).floor(), top + rh + 6.0],
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
        let w = batch.width;
        let [rx, ry, rw, rh] = layout.region;
        let lower_region = [rx + layout.offset[0], ry + layout.offset[1], rw, rh];
        let window = self
            .renderer
            .as_ref()
            .map(|r| HudBatch::new([r.config.width, r.config.height]));
        // The slot at `p`: the hotbar's, the inventory's, or one of vanilla's part (found
        // where it is on the window).
        let slot_at = |game: &Self, p: [f32; 2]| {
            lower_hotbar_slot(w, BAND, p)
                .or_else(|| Self::grid_slot(layout.inventory, p))
                .or_else(|| {
                    let window = window.as_ref().filter(|_| inside(lower_region, p))?;
                    let gui = [p[0] - layout.offset[0], p[1] - layout.offset[1]];
                    game.slot_at(window, gui[0], gui[1])
                })
        };
        let click = |game: &mut Self, slot: usize| game.container_click(slot, 0, Input::Pickup);
        for touch in touches {
            match *touch {
                Touch::Down([x, y]) => {
                    let p = [x / layout.scale, y / layout.scale];
                    CONTAINER.with_borrow_mut(|s| (s.at, s.pad_active) = (p, false));
                    match slot_at(self, p) {
                        Some(slot) => self.slot_down(Some(slot), p, click),
                        // Vanilla's buttons and fields: clicked where they are on the window.
                        None if inside(lower_region, p) => {
                            let gui = [p[0] - layout.offset[0], p[1] - layout.offset[1]];
                            self.click_window_at(events, id, gui);
                        }
                        None => self.slot_down(None, p, click),
                    }
                }
                Touch::Move([x, y]) => {
                    let p = [x / layout.scale, y / layout.scale];
                    CONTAINER.with_borrow_mut(|s| s.at = p);
                    Self::slot_move(p);
                }
                Touch::Up([x, y]) => {
                    let p = [x / layout.scale, y / layout.scale];
                    let slot = slot_at(self, p);
                    self.slot_up(slot, click);
                }
            }
            if !self.screen_on_second_screen() || self.screen != Screen::Inventory {
                return true;
            }
        }
        let right = |game: &mut Self, slot: usize| game.container_click(slot, 1, Input::Pickup);
        self.slot_hold(click, right);
        self.draw_lower_hotbar(&mut batch, BAND);
        // Vanilla's screen, at the window's size with the pointer away (no hover, no tooltip,
        // no carried stack), cut to the container's part and moved here.
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
        // The inventory, in vanilla's slots.
        let [ix, iy] = layout.inventory;
        for i in 0..27 {
            let (x, y) = (ix + (i % 9) as f32 * 18.0, iy + (i / 9) as f32 * 18.0);
            slot_frame(&mut batch, &self.gui.atlas, [x, y, 18.0, 18.0]);
            if let Some(stack) = self.inventory.get(9 + i).cloned().flatten() {
                self.draw_item(&mut batch, &stack, x + 1.0, y + 1.0);
            }
        }
        // The controller's slot, and the carried stack on it or where the last tap was.
        let (active, focus) = CONTAINER.with_borrow(|s| (s.pad_active, s.pad));
        if active && let Some(rect) = self.container_slot_rect(&layout, w, focus) {
            super::menu::highlight(&mut batch, &self.gui.atlas, rect);
            let at = [rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0];
            CONTAINER.with_borrow_mut(|s| s.at = at);
        }
        if let Some(stack) = self.carried.clone() {
            let [x, y] = CONTAINER.with_borrow(|s| s.at);
            self.draw_item(&mut batch, &stack, x - 8.0, y - 8.0);
        }
        self.present_lower(&batch, BACKGROUND);
        true
    }

    /// The inventory grid's slot at `p`.
    fn grid_slot([ix, iy]: [f32; 2], p: [f32; 2]) -> Option<usize> {
        let (c, r) = ((p[0] - ix) / 18.0, (p[1] - iy) / 18.0);
        (c >= 0.0 && r >= 0.0 && c < 9.0 && r < 3.0).then(|| 9 + r as usize * 9 + c as usize)
    }

    fn container_click(&mut self, slot: usize, button: i32, input: Input) {
        CONTAINER.with_borrow_mut(|s| s.focus = Some(slot));
        self.menu_click(Some(slot), button, input);
    }

    /// The slots as a controller moves over them, top to bottom: the hotbar, the container's
    /// rows (by where vanilla puts them), the inventory's three rows.
    fn container_pad_rows(&self) -> Vec<Vec<usize>> {
        let mut rows: Vec<Vec<usize>> = vec![(0..9).collect()];
        let mut container: BTreeMap<i32, Vec<(i32, usize)>> = BTreeMap::new();
        for slot in self.slots().into_iter().filter(|&s| s >= 36) {
            let (x, y) = self.slot_position(slot);
            container
                .entry(y.round() as i32)
                .or_default()
                .push((x.round() as i32, slot));
        }
        for (_, mut row) in container {
            row.sort();
            rows.push(row.into_iter().map(|(_, s)| s).collect());
        }
        rows.extend((0..3).map(|r| (9 + r * 9..18 + r * 9).collect()));
        rows
    }

    /// Where a slot of `pad_rows` is on the lower screen.
    fn container_slot_rect(
        &self,
        layout: &Layout,
        width: f32,
        (row, col): (usize, usize),
    ) -> Option<[f32; 4]> {
        let rows = self.container_pad_rows();
        let slot = *rows.get(row)?.get(col)?;
        Some(match slot {
            0..9 => lower_hotbar_rect(width, slot),
            9..36 => {
                let i = slot - 9;
                let [ix, iy] = layout.inventory;
                [
                    ix + (i % 9) as f32 * 18.0,
                    iy + (i / 9) as f32 * 18.0,
                    18.0,
                    18.0,
                ]
            }
            _ => {
                let (sx, sy) = self.slot_position(slot);
                let window = self
                    .renderer
                    .as_ref()
                    .map(|r| HudBatch::new([r.config.width, r.config.height]))?;
                let (ox, oy) = self.inventory_origin(&window);
                [
                    ox + sx - 1.0 + layout.offset[0],
                    oy + sy - 1.0 + layout.offset[1],
                    18.0,
                    18.0,
                ]
            }
        })
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

    /// The window while a container is open: its block large, its title, the chosen item's
    /// name on a dark bar and the controls, as New 3DS Edition's.
    pub(super) fn draw_container_upper(&mut self, batch: &mut HudBatch) {
        let menu = self.remote_menu().1;
        let title = self
            .remote_menu_title()
            .map(|t| t.plain())
            .unwrap_or_else(|| self.translate("container.inventory", "Inventory"));
        let (w, h) = (batch.width, batch.height);
        let (cx, cy) = ((w / 2.0).floor(), (h * 0.33).floor());
        let start = batch.vertices.len();
        self.draw_item(
            batch,
            &ItemStack::new(container_icon(&menu), 1),
            cx - 8.0,
            cy - 8.0,
        );
        hud::transform_since(batch, start, [cx, cy], 4.0, [0.0, 0.0]);
        let atlas = &self.gui.atlas;
        centered(batch, atlas, &title, [0.0, cy + 40.0, w], u32::MAX, true);
        batch.fill(atlas, 0.0, cy + 52.0, w, 14.0, 0xDC30_3030);
        let focus = CONTAINER.with_borrow(|s| s.focus);
        let stack = self.carried.clone().or_else(|| match focus {
            Some(slot @ 0..36) => self.inventory.get(slot).cloned().flatten(),
            Some(slot) => self.slot_view(slot),
            None => None,
        });
        let name = stack.map_or(title, |s| {
            item_tooltip::name_line(&s, &|key| self.pack.names.display_get(key))
        });
        let hint = self.pad_hint(
            "{south} Take All    {north} Take Half    {west} Quick Move    {lb}/{rb} Hotbar    {east} Exit",
        );
        let atlas = &self.gui.atlas;
        centered(batch, atlas, &name, [0.0, cy + 55.0, w], u32::MAX, true);
        centered(batch, atlas, &hint, [0.0, cy + 70.0, w], 0xFFCC_CCCC, true);
    }
}
