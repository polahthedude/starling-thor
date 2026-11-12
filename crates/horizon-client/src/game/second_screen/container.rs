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
    /// While the window's recipe book is open the controller drives it (Select hands the pad
    /// back to the slots): the book's control it is on (`book_pad_targets`), and a press to
    /// make on the window at the next frame.
    book_pad_off: bool,
    book_focus: usize,
    book_press: Option<([f32; 2], bool)>,
    /// The control a recipe was last placed from: A on it again takes the craft.
    book_placed: Option<usize>,
    /// The creative screen's slot the controller pressed, in window pixels, for the next frame.
    creative_press: Option<[f32; 2]>,
    /// A finger on the creative grid: where it came down (lower screen's pixels), the height
    /// it last scrolled from, and whether it has swiped. A tap that never swiped presses the
    /// slot when it lifts.
    swipe: Option<([f32; 2], f32, bool)>,
    /// The swipe's speed in rows a second (smoothed) and when it was last measured; after the
    /// finger lifts at speed, the grid keeps going at `fling`, slowing, until it stops.
    swipe_speed: (f32, Option<std::time::Instant>),
    fling: (f32, Option<std::time::Instant>),
    /// The creative grid's first row as last drawn (each new one ticks the haptics), and the
    /// scrollbar thumb's last position, time and speed (scroll a second) while held.
    row: usize,
    thumb: (f32, Option<std::time::Instant>, f32),
}

thread_local! {
    static CONTAINER: RefCell<State> = RefCell::new(State::default());
}

/// Kept around vanilla's screen on the lower screen, in its GUI pixels.
const MARGIN: f32 = 1.0;

/// Vanilla's screen as large as fits, in quarter steps. Off: the largest whole scale.
const FRACTIONAL_FIT: bool = true;

/// The block a menu type stands for, as the window's icon.
fn container_icon(menu: &str) -> &'static str {
    match menu.trim_start_matches("minecraft:") {
        m if m.starts_with("generic_9x") => "minecraft:chest",
        "generic_3x3" => "minecraft:dispenser",
        "crafter_3x3" => "minecraft:crafter",
        "crafting" => "minecraft:crafting_table",
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
        if self.screen == Screen::Creative {
            // Vanilla's creative screen and its tabs, which stand 28 above and below it.
            let (ox, oy) = self.inventory_origin(window);
            return Some([ox, oy - 28.0, Creative::WIDTH, Creative::HEIGHT + 56.0]);
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
        // Whole scales keep vanilla's art crisp but can leave a sixth of the screen empty, so
        // the fit is taken to the nearest quarter below (`FRACTIONAL_FIT`: off, whole ones).
        let scale = if FRACTIONAL_FIT {
            ((fit * 4.0).floor() / 4.0).max(1.0)
        } else {
            fit.floor().max(1.0)
        };
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
        // Docked on the window: nothing of the menu here, and a tap puts the keyboard away.
        if self.dock_amount() > 0.02 {
            if touches.iter().any(|t| matches!(t, Touch::Down(_))) {
                crate::platform::hide();
            }
            let mut blank = HudBatch::new(size);
            blank.scale = layout.scale;
            self.present_lower(&mut blank, MENU_BACKGROUND);
            return true;
        }
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
        // The creative screen is the window's mouse all over (its tabs, scrollbar and slots as
        // vanilla's), so no slot is the frame's own there.
        let creative = self.screen == Screen::Creative;
        // The creative grid's five rows of nine, where a swipe scrolls.
        let grid_area = window
            .as_ref()
            .map(|w| {
                let (ox, oy) = self.inventory_origin(w);
                [
                    ox + 9.0 + layout.offset[0],
                    oy + 18.0 + layout.offset[1],
                    162.0,
                    90.0,
                ]
            })
            .unwrap_or_default();
        let slot_at = |game: &Self, p: [f32; 2]| {
            let window = window
                .as_ref()
                .filter(|_| inside(lower_region, p) && !creative)?;
            let [x, y] = to_window(p);
            game.slot_at(window, x, y)
        };
        if let Some(at) = CONTAINER.with_borrow_mut(|s| s.creative_press.take()) {
            self.window_mouse(events, id, at, Some(ElementState::Pressed));
            self.window_mouse(events, id, at, Some(ElementState::Released));
        }
        // The controller's press on the window's recipe book.
        if let Some((at, again)) = CONTAINER.with_borrow_mut(|s| s.book_press.take()) {
            // A again on the recipe just placed: its craft is in the result slot, take it.
            if again && self.slot_view(200).is_some() {
                CONTAINER.with_borrow_mut(|s| s.book_placed = None);
                self.container_click(200, 0, Input::QuickMove);
            } else {
                // The book's own click, at the control, not a mouse the window has to catch up
                // with.
                self.cursor = (at[0], at[1]);
                self.recipe_book_click(false);
                self.recipe_book_release();
                CONTAINER.with_borrow_mut(|s| s.book_placed = Some(s.book_focus));
            }
        }
        let click = |game: &mut Self, slot: usize| game.container_click(slot, 0, Input::Pickup);
        let pressing = || CONTAINER.with_borrow(|s| s.pressing);
        let set_pressing = |on: bool| CONTAINER.with_borrow_mut(|s| s.pressing = on);
        for touch in touches {
            match *touch {
                Touch::Down([x, y]) => {
                    let p = [x / layout.scale, y / layout.scale];
                    CONTAINER.with_borrow_mut(|s| (s.at, s.pad_active) = (p, false));
                    // The creative grid: a swipe scrolls it, a tap takes what is under the
                    // finger once it lifts.
                    // A finger down catches a coasting grid.
                    CONTAINER.with_borrow_mut(|s| s.fling = (0.0, None));
                    if creative
                        && inside(grid_area, p)
                        && self.creative_screen.can_scroll(&self.pack.names)
                    {
                        CONTAINER.with_borrow_mut(|s| {
                            s.swipe = Some((p, p[1], false));
                            s.swipe_speed = (0.0, Some(std::time::Instant::now()));
                        });
                        continue;
                    }
                    if creative && let Some(window) = window.as_ref() {
                        // The item under the finger is the one the window shows.
                        let [x, y] = to_window(p);
                        let slot = self.slot_at(window, x, y);
                        CONTAINER.with_borrow_mut(|s| s.focus = slot.or(s.focus));
                    }
                    match slot_at(self, p) {
                        Some(slot) => self.slot_down(Some(slot), p, click),
                        // Vanilla's buttons, lists and fields: pressed where they are on the
                        // window, held while the finger is down.
                        None if inside(lower_region, p) => {
                            set_pressing(true);
                            self.android_touch_haptic(6);
                            CONTAINER.with_borrow_mut(|s| {
                                s.thumb = (self.creative_screen.scroll, Some(std::time::Instant::now()), 0.0);
                            });
                            let state = Some(ElementState::Pressed);
                            self.window_mouse(events, id, to_window(p), state);
                        }
                        None if creative => {}
                        None => self.slot_down(None, p, click),
                    }
                }
                Touch::Move([x, y]) => {
                    let p = [x / layout.scale, y / layout.scale];
                    CONTAINER.with_borrow_mut(|s| s.at = p);
                    if let Some((start, last, moved)) = CONTAINER.with_borrow(|s| s.swipe) {
                        let moved = moved || (p[0] - start[0]).hypot(p[1] - start[1]) > 6.0;
                        if moved {
                            // A row is 18 pixels: the grid follows the finger.
                            let rows = f64::from((p[1] - last) / 18.0);
                            self.creative_screen.wheel(rows, &self.pack.names);
                            // Its speed, smoothed over the last few moves.
                            let now = std::time::Instant::now();
                            CONTAINER.with_borrow_mut(|s| {
                                if let Some(t) = s.swipe_speed.1 {
                                    let dt = now.duration_since(t).as_secs_f32().max(0.004);
                                    let speed = rows as f32 / dt;
                                    s.swipe_speed.0 = s.swipe_speed.0 * 0.6 + speed * 0.4;
                                }
                                s.swipe_speed.1 = Some(now);
                            });
                        }
                        let last = if moved { p[1] } else { last };
                        CONTAINER.with_borrow_mut(|s| s.swipe = Some((start, last, moved)));
                        continue;
                    }
                    if pressing() {
                        self.window_mouse(events, id, to_window(p), None);
                        if creative && self.creative_screen.scrolling {
                            let (now, scroll) = (std::time::Instant::now(), self.creative_screen.scroll);
                            CONTAINER.with_borrow_mut(|s| {
                                if let Some(t) = s.thumb.1 {
                                    let dt = now.duration_since(t).as_secs_f32().max(0.004);
                                    let v = (scroll - s.thumb.0) / dt;
                                    s.thumb.2 = s.thumb.2 * 0.6 + v * 0.4;
                                }
                                s.thumb.0 = scroll;
                                s.thumb.1 = Some(now);
                            });
                        }
                    } else {
                        Self::slot_move(p);
                    }
                }
                Touch::Up([x, y]) => {
                    let p = [x / layout.scale, y / layout.scale];
                    if let Some((start, _, moved)) = CONTAINER.with_borrow_mut(|s| s.swipe.take()) {
                        if moved {
                        } else {
                                    (0.0, None)
                                };
                            });
                        } else {
                            let at = to_window(start);
                            self.android_touch_haptic(6);
                            self.window_mouse(events, id, at, Some(ElementState::Pressed));
                            self.window_mouse(events, id, at, Some(ElementState::Released));
                        }
                        continue;
                    }
                    if pressing() {
                        set_pressing(false);
                        if creative && self.creative_screen.scrolling {
                            CONTAINER.with_borrow_mut(|s| s.thumb = (0.0, None, 0.0));
                        }
                        let state = Some(ElementState::Released);
                        self.window_mouse(events, id, to_window(p), state);
                        if (creative || self.station_typing())
                            && crate::platform::field_at(to_window(p))
                        {
                            self.android_show_keyboard();
                        }
                    } else {
                        let slot = slot_at(self, p);
                        self.slot_up(slot, click);
                    }
                }
            }
            if !self.screen_on_second_screen()
                || !matches!(self.screen, Screen::Inventory | Screen::Creative)
            {
                return true;
            }
        }
        // A tick for every row the creative grid passes, however it was moved.
        if creative {
            let row = self.creative_screen.first_row(&self.pack.names);
            if CONTAINER.with_borrow_mut(|s| std::mem::replace(&mut s.row, row)) != row {
                self.android_touch_haptic(4);
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
        super::LOWER_PASS.set(true);
        if creative {
            self.draw_creative(&mut batch);
        } else {
            self.draw_inventory(&mut batch);
        }
        super::LOWER_PASS.set(false);
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
        if self.screen == Screen::Creative {
            if let Some(taken) = self.creative_pad_input(&window, button) {
                return taken;
            }
        } else if self.book_pad_input(button) {
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

    /// A controller button on vanilla's creative screen that isn't a move over the slots: the
    /// triggers change tab, A presses the slot as the window's mouse, X (Quick Move) shift-
    /// presses it. None leaves the button to the slots' own handling.
    fn creative_pad_input(
        &mut self,
        window: &HudBatch,
        button: horizon_client::gamepad::Button,
    ) -> Option<bool> {
        use horizon_client::gamepad::Button as B;
        match button {
            B::LeftTrigger | B::RightTrigger => {
                let n = self.creative_screen.tab_headers().len().max(1);
                let at = self.creative_screen.selected;
                let next = if button == B::RightTrigger {
                    (at + 1) % n
                } else {
                    (at + n - 1) % n
                };
                self.creative_screen.select(next);
                Some(true)
            }
            B::South => {
                let rows = self.container_pad_rows();
                let (row, col) = CONTAINER.with_borrow(|s| s.pad);
                let row = row.min(rows.len().saturating_sub(1));
                let slot = rows
                    .get(row)
                    .and_then(|r| r.get(col.min(r.len().saturating_sub(1))))
                    .copied()?;
                let (ox, oy) = self.inventory_origin(window);
                let (sx, sy) = self.slot_position(slot);
                CONTAINER.with_borrow_mut(|s| {
                    s.pad_active = true;
                    s.focus = Some(slot);
                    s.creative_press = Some([ox + sx + 8.0, oy + sy + 8.0]);
                });
                Some(true)
            }
            // The other buttons would click a container's menu: not this screen's.
            B::North | B::West => Some(true),
            _ => None,
        }
    }

    /// The stack in a creative screen slot: an item tab's grid slots show what the tab lists at
    /// the scrolled row, the others are the game's.
    fn creative_stack(&mut self, slot: usize) -> Option<ItemStack> {
        if slot >= Creative::DISPLAY {
            let first = self.creative_screen.first_row(&self.pack.names);
            return self
                .creative_screen
                .shown(&self.pack.names, first * 9 + slot - Creative::DISPLAY);
        }
        self.slot_mut(slot).clone()
    }

    /// The window while the creative screen is open: the chosen item large (spun by a finger
    /// on the window), the tab's title and the item's name, and the controls.
    pub(super) fn draw_creative_container_upper(&mut self, batch: &mut HudBatch) {
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
        let stack = self
            .carried
            .clone()
            .or_else(|| focus.and_then(|slot| self.creative_stack(slot)));
        let (w, h) = (batch.width, batch.height);
        let center = [(w / 2.0).floor(), (h * 0.40).floor()];
        // A docked GUI takes the showcase's side; the tooltip would sit where it goes.
        let (center, dock) = self.dock_place(batch, center);
        let shown = stack
            .clone()
            .unwrap_or_else(|| ItemStack::new("minecraft:chest", 1));
        self.draw_showcase(batch, &shown, center);
        if let Some(dock) = dock {
            self.draw_docked(batch, dock);
        } else if let Some(stack) = &stack {
            self.draw_showcase_tooltip(batch, stack, true, center);
        }
        let title = {
            let tabs = self.creative_screen.tab_headers();
            let tab = &tabs[self.creative_screen.selected.min(tabs.len() - 1)];
            self.creative_tab_title(tab)
        };
        let name =
            stack.map(|s| item_tooltip::name_line(&s, &|key| self.pack.names.display_get(key)));
        let (heading, name) = match &name {
            Some(name) => (Some(title.as_str()), name.as_str()),
            None => (None, title.as_str()),
        };
        let hint = self.pad_hint("{south} Take    {lt}/{rt} Tab    {east} Exit");
        self.draw_upper_caption(batch, heading, Some(name), &hint);
    }

    /// The window's open recipe book's controls a controller can be on (not the book button,
    /// which the lower screen has), in GUI pixels; none while the book is closed.
    fn book_pad_targets(&self) -> Vec<[f32; 4]> {
        let Some(window) = self
            .renderer
            .as_ref()
            .map(|r| HudBatch::new([r.config.width, r.config.height]))
        else {
            return Vec::new();
        };
        if self.open_recipe_book_rect(&window).is_none() {
            return Vec::new();
        }
        self.recipe_book_targets()
            .into_iter()
            .skip(1)
            .map(|t| t.rect)
            .collect()
    }

    /// A controller button while the window's recipe book is open and has the pad: the
    /// directions move to the next control that way, A presses it, Select hands the pad to
    /// the slots (and back). True when taken.
    fn book_pad_input(&mut self, button: horizon_client::gamepad::Button) -> bool {
        use horizon_client::gamepad::Button as B;
        let targets = self.book_pad_targets();
        if targets.is_empty() {
            return false;
        }
        let (off, focus) = CONTAINER.with_borrow(|s| (s.book_pad_off, s.book_focus));
        if button == B::Select {
            CONTAINER.with_borrow_mut(|s| s.book_pad_off = !off);
            return true;
        }
        if off {
            return false;
        }
        let focus = focus.min(targets.len() - 1);
        let center = |r: [f32; 4]| [r[0] + r[2] / 2.0, r[1] + r[3] / 2.0];
        let dir = match button {
            B::DPadUp => [0.0, -1.0],
            B::DPadDown => [0.0, 1.0],
            B::DPadLeft => [-1.0, 0.0],
            B::DPadRight => [1.0, 0.0],
            B::South => {
                let at = center(targets[focus]);
                CONTAINER.with_borrow_mut(|s| {
                    s.book_press = Some((at, s.book_placed == Some(focus)));
                });
                return true;
            }
            // B and Start go back, as ever; the rest are the slots'.
            _ => return false,
        };
        let from = center(targets[focus]);
        // The nearest control that way, straighter ones first.
        let next = targets
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != focus)
            .filter_map(|(i, &r)| {
                let c = center(r);
                let (dx, dy) = (c[0] - from[0], c[1] - from[1]);
                let along = dx * dir[0] + dy * dir[1];
                let across = (dx * dir[1] + dy * dir[0]).abs();
                (along > 0.5).then_some((i, along + 2.0 * across))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i);
        CONTAINER.with_borrow_mut(|s| {
            s.book_focus = next.unwrap_or(focus);
            s.book_placed = None;
        });
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
        // The controller's place in the book, while it has the pad.
        let book_pad = book.is_some() && !CONTAINER.with_borrow(|s| s.book_pad_off);
        if book.is_none() {
            CONTAINER.with_borrow_mut(|s| (s.book_pad_off, s.book_focus) = (false, 0));
        } else if book_pad {
            let targets = self.book_pad_targets();
            let focus = CONTAINER.with_borrow(|s| s.book_focus);
            if let Some(&rect) = targets.get(focus.min(targets.len().saturating_sub(1))) {
                super::menu::highlight(batch, &self.gui.atlas, rect);
            }
        }
        // The recipe under the finger in the book is shown too, over the slot's.
        let stack = book
            .and_then(|_| self.recipe_ui.hovered.clone())
            .or(stack);
        let left = book_slide(book.map(|[x, _, bw, _]| x + bw + 4.0));
        let span = [left, w - left];
        let center = [(left + span[1] / 2.0).floor(), (h * 0.40).floor()];
        let (center, dock) = self.dock_place(batch, center);
        match (&stack, menu.as_str()) {
            // The player's own inventory: the player, not a block.
            (None, "inventory") => self.draw_showcase_player(batch, center),
            _ => {
                let shown = stack
                    .clone()
                    .unwrap_or_else(|| ItemStack::new(container_icon(&menu), 1));
                self.draw_showcase(batch, &shown, center);
                if let Some(dock) = dock {
                    self.draw_docked(batch, dock);
                } else if let Some(stack) = &stack {
                    self.draw_showcase_tooltip(batch, stack, false, center);
                }
            }
        }
        let name =
            stack.map(|s| item_tooltip::name_line(&s, &|key| self.pack.names.display_get(key)));
        let (heading, name) = match &name {
            Some(name) => (Some(title.as_str()), name.as_str()),
            None => (None, title.as_str()),
        };
        let hint = if book_pad {
            self.pad_hint("D-pad Choose    {south} Select    {select} Slots    {east} Exit")
        } else if book.is_some() {
            self.pad_hint(
                "{south} Take All    {north} Take Half    {west} Quick Move    {select} Book    {east} Exit",
            )
        } else {
            self.pad_hint(
                "{south} Take All    {north} Take Half    {west} Quick Move    {east} Exit",
            )
        };
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
