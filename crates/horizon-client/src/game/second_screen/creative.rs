//! The dual screen's creative screen, laid out across the two screens as New 3DS Edition's: on
//! the lower screen the hotbar, the tab's title, its items in a grid with a scrollbar, and the
//! tab bar (the inventory's at the right end); on the window, over the running world, the
//! chosen item large with its name. Styled as Legacy Console Edition's. The tabs are Java's
//! creative tabs, which hold every item of this version; the search and saved hotbars tabs need
//! a keyboard and the controller and aren't shown. A tapped item fills the selected hotbar slot
//! with a full stack (the 3DS's Take), sent to a server as the creative screen sends it.
//!
//! The grid scrolls by the pixel, easing to where a swipe, the scrollbar, its arrows or the
//! controller send it; an item is taken when the finger lifts without having swiped.
use super::menu::{ON_FRAME, centered, content_top, highlight, inside, panel, slot_frame};
use super::*;
use crate::second_screen::Touch;
use horizon_client::{item_tooltip, touch::dual_menu};

/// The grid's cells: the hotbar's slots' pitch, each column under a hotbar slot.
const SLOT: f32 = dual_menu::CELL;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Clone, Copy)]
enum Finger {
    /// On the grid: where it came down, the scroll then, and whether it has swiped.
    Grid { y: f32, from: f32, swiped: bool },
    /// Holding the scrollbar, this far below the thumb's middle (0 when it jumped there).
    Track { grab: f32 },
    /// On the tab bar: where it came down, the tabs' scroll then, and whether it has swiped.
    Tabs { x: f32, from: f32, swiped: bool },
}

#[derive(Default)]
struct State {
    /// Each tab's scroll in pixels, by tab id: where it is and where it eases to.
    scroll: HashMap<String, (f32, f32)>,
    /// The item last taken, shown large on the window.
    chosen: Option<ItemStack>,
    /// Its place in the tab's list, highlighted; where the controller is on the grid.
    index: Option<usize>,
    at: [f32; 2],
    /// The grid's height as last drawn, for the controller's scrolling.
    shown: f32,
    finger: Option<Finger>,
    /// The last frame, for the easing.
    last: Option<Instant>,
    /// How far the tab bar is scrolled, and the tab it was last kept in sight for.
    tab_scroll: f32,
    tab_shown: Option<usize>,
}

thread_local! {
    static CREATIVE: RefCell<State> = RefCell::new(State::default());
}

impl Game<'_> {
    /// Java's item tabs shown as tabs: not the search, saved hotbars or inventory ones.
    fn creative_item_tabs(&self) -> Vec<usize> {
        let tabs = &self.creative_screen.tabs;
        (0..tabs.len())
            .filter(|&i| !tabs[i].search && !tabs[i].hotbar && !tabs[i].inventory)
            .collect()
    }

    fn creative_inventory_tab(&self) -> Option<usize> {
        self.creative_screen.tabs.iter().position(|t| t.inventory)
    }

    /// This tab's scroll: where it is and where it eases to.
    fn creative_scroll(&self) -> (f32, f32) {
        let id = &self.creative_screen.tab().id;
        CREATIVE.with_borrow(|s| s.scroll.get(id).copied().unwrap_or_default())
    }

    /// Ease this tab's scroll to `target`, or jump there with `now`.
    fn creative_scroll_to(&mut self, target: f32, now: bool) {
        let id = self.creative_screen.tab().id.clone();
        CREATIVE.with_borrow_mut(|s| {
            let entry = s.scroll.entry(id).or_default();
            *entry = (if now { target } else { entry.0 }, target);
        });
    }

    /// A controller button on the creative screen; true when taken. The triggers change tab,
    /// the shoulders the selected hotbar slot. On an item tab, as New 3DS Edition's: the pad
    /// moves over the items, A puts a stack in the selected hotbar slot, Y clears the hotbar;
    /// the inventory's tab as the survival inventory.
    pub(super) fn creative_pad(&mut self, button: horizon_client::gamepad::Button) -> bool {
        use horizon_client::gamepad::Button as B;
        let tabs = self.creative_item_tabs();
        let selected = self.creative_screen.selected;
        if super::menu::hotbar_shoulder(self, button) {
            return true;
        }
        if matches!(button, B::LeftTrigger | B::RightTrigger) {
            let mut all = tabs.clone();
            all.extend(self.creative_inventory_tab());
            if all.is_empty() {
                return true;
            }
            let at = all.iter().position(|&i| i == selected).unwrap_or(0);
            let next = if button == B::RightTrigger {
                (at + 1) % all.len()
            } else {
                (at + all.len() - 1) % all.len()
            };
            self.creative_screen.select(all[next]);
            CREATIVE.with_borrow_mut(|s| s.index = None);
            return true;
        }
        if !tabs.contains(&selected) {
            return self.legacy_inventory_pad(button);
        }
        let items = self.creative_screen.items(&self.pack.names);
        let (_, target) = self.creative_scroll();
        let (index, shown) = CREATIVE.with_borrow(|s| (s.index, s.shown.max(SLOT)));
        let at = index.unwrap_or((target / SLOT).ceil() as usize * 9);
        let last = items.len().saturating_sub(1);
        let moved = match button {
            B::DPadLeft => Some(at.saturating_sub(1)),
            B::DPadRight => Some((at + 1).min(last)),
            B::DPadUp => Some(at.saturating_sub(9)),
            B::DPadDown => Some((at + 9).min(last)),
            _ => None,
        };
        if let Some(next) = moved {
            // Scrolled to keep it in sight.
            let top = (next / 9) as f32 * SLOT;
            if top < target {
                self.creative_scroll_to(top, false);
            } else if top + SLOT > target + shown {
                self.creative_scroll_to(top + SLOT - shown, false);
            }
            let stack = items.get(next).filter(|s| s.count > 0).cloned();
            CREATIVE.with_borrow_mut(|s| {
                s.index = Some(next);
                if stack.is_some() {
                    s.chosen = stack;
                }
            });
            return true;
        }
        match button {
            B::South => {
                if let Some(stack) = items.get(at).filter(|s| s.count > 0) {
                    self.creative_take(stack.clone(), at);
                }
            }
            // The 3DS's Clear Hotbar.
            B::North => {
                for slot in self.inventory.iter_mut().take(9) {
                    *slot = None;
                }
                if self.remote.is_some() {
                    self.send_creative_slots();
                }
            }
            _ => return false,
        }
        true
    }

    /// One frame of the lower screen while the creative screen is open.
    pub(super) fn creative_frame(&mut self, size: [u32; 2], touches: &[Touch]) {
        let tabs = self.creative_item_tabs();
        let inventory = self.creative_inventory_tab();
        let selected = self.creative_screen.selected;
        // The search and saved hotbars tabs aren't here: their place goes to the first tab.
        if !tabs.contains(&selected)
            && Some(selected) != inventory
            && let Some(&first) = tabs.first()
        {
            self.creative_screen.select(first);
        }
        // The panel's scale, the inventory's tab fitting: the tabs scroll.
        let band = touch::dual_panel(0.0, 0.0).band;
        let scale = dual_menu::inventory_scale(size, 0, band);
        let mut batch = lower_batch_at(size, scale);
        let (w, h, scale) = (batch.width, batch.height, batch.scale);
        let band = touch::dual_panel(w, h).band;
        let dims = [w, h, band];
        for touch in touches {
            match *touch {
                Touch::Down([x, y]) => {
                    self.creative_tap(&tabs, inventory, dims, [x / scale, y / scale]);
                }
                Touch::Move([x, y]) => self.creative_drag(dims, tabs.len(), [x / scale, y / scale]),
                Touch::Up([x, y]) => {
                    self.creative_release(dims, tabs.len(), [x / scale, y / scale]);
                }
            }
            if self.screen != Screen::Creative {
                return;
            }
        }
        self.creative_ease();
        self.slot_hold(Self::legacy_click, Self::legacy_right_click);
        let selected = self.creative_screen.selected;
        let chosen = tabs.iter().position(|&i| i == selected);
        let tab_scroll = Self::creative_tabs_follow(dims, tabs.len(), chosen);
        self.draw_menu_ground(&mut batch, band, tabs.len());
        match chosen {
            Some(_) => self.draw_creative_grid(&mut batch, band, tabs.len()),
            None => self.draw_legacy_inventory(&mut batch, band),
        }
        let icons: Vec<String> = tabs
            .iter()
            .map(|&i| self.creative_screen.tabs[i].icon.clone())
            .collect();
        let icons: Vec<&str> = icons.iter().map(String::as_str).collect();
        self.draw_lower_tabs(&mut batch, &icons, chosen, tab_scroll);
        if let Some(stack) = self.carried.clone() {
            let [x, y] = CREATIVE.with_borrow(|s| s.at);
            self.draw_item(&mut batch, &stack, x - 8.0, y - 8.0);
        }
        self.present_lower(&mut batch, BACKGROUND);
    }

    /// The tab bar's scroll, kept in range, and moved to bring `chosen` into sight when it
    /// changed (a controller's trigger, a tap on the window).
    fn creative_tabs_follow([w, h, _]: [f32; 3], tabs: usize, chosen: Option<usize>) -> f32 {
        let max = dual_menu::tabs_scroll_max(w, h, tabs);
        CREATIVE.with_borrow_mut(|s| {
            if chosen != s.tab_shown {
                s.tab_shown = chosen;
                if let Some(i) = chosen {
                    let (cats, _, view) = dual_menu::tabs_scrolled(w, h, tabs, s.tab_scroll);
                    let [x, _, size, _] = cats[i];
                    if x < view[0] {
                        s.tab_scroll -= view[0] - x;
                    } else if x + size > view[0] + view[2] {
                        s.tab_scroll += x + size - view[0] - view[2];
                    }
                }
            }
            s.tab_scroll = s.tab_scroll.clamp(0.0, max);
            s.tab_scroll
        })
    }

    /// The scroll eased toward where it is going, about 0.15 s to arrive.
    fn creative_ease(&mut self) {
        let now = Instant::now();
        let id = self.creative_screen.tab().id.clone();
        CREATIVE.with_borrow_mut(|s| {
            let dt = s.last.map_or(0.0, |t| (now - t).as_secs_f32().min(0.1));
            s.last = Some(now);
            if let Some((at, target)) = s.scroll.get_mut(&id) {
                let k = 1.0 - (-18.0 * dt).exp();
                *at += (*target - *at) * k;
                if (*target - *at).abs() < 0.5 {
                    *at = *target;
                }
            }
        });
    }

    /// The grid as laid out for `tabs` item tabs, this tab's items, how far it scrolls and
    /// where it is (both kept in range).
    fn creative_layout(
        &mut self,
        [w, h, band]: [f32; 3],
        tabs: usize,
    ) -> (dual_menu::Grid, std::sync::Arc<[ItemStack]>, f32, f32) {
        // Inside the frame: from its top to its bottom (the tab bar's strip, 2, and FRAME).
        let (_, _, view) = dual_menu::tabs_scrolled(w, h, tabs, 0.0);
        let bottom = view[1] - 4.0 - dual_menu::FRAME;
        let grid = dual_menu::grid(w, content_top(band), bottom);
        let items = self.creative_screen.items(&self.pack.names);
        let rows = items.len().div_ceil(9);
        let max = rows.saturating_sub(grid.rows) as f32 * SLOT;
        let (at, target) = self.creative_scroll();
        if at > max || target > max {
            let id = self.creative_screen.tab().id.clone();
            CREATIVE.with_borrow_mut(|s| s.scroll.insert(id, (at.min(max), target.min(max))));
        }
        (grid, items, max, at.min(max))
    }

    /// The grid's slots' area.
    fn creative_grid_area(grid: &dual_menu::Grid) -> [f32; 4] {
        let [ox, oy] = grid.origin;
        [ox, oy, 9.0 * SLOT, grid.rows as f32 * SLOT]
    }

    /// The scrollbar held at `y`: the thumb's middle there.
    fn creative_track(&mut self, grid: &dual_menu::Grid, max: f32, y: f32) {
        let [_, ty, _, th] = grid.track;
        let thumb = dual_menu::SCROLLER[1];
        let v = ((y - ty - thumb / 2.0) / (th - thumb).max(1.0)).clamp(0.0, 1.0);
        self.creative_scroll_to(v * max, true);
    }

    /// A finger coming down on the lower screen.
    fn creative_tap(
        &mut self,
        tabs: &[usize],
        inventory: Option<usize>,
        [w, h, band]: [f32; 3],
        p: [f32; 2],
    ) {
        CREATIVE.with_borrow_mut(|s| (s.at, s.finger) = (p, None));
        let scroll = CREATIVE.with_borrow(|s| s.tab_scroll);
        let (_, inventory_tab, view) = dual_menu::tabs_scrolled(w, h, tabs.len(), scroll);
        if inside(inventory_tab, p) {
            if let Some(i) = inventory
                && self.creative_screen.selected != i
            {
                self.creative_screen.select(i);
                self.android_touch_haptic(8);
            }
            return;
        }
        // The tabs: chosen when the finger lifts without swiping them along.
        if inside(view, p) {
            let finger = Finger::Tabs {
                x: p[0],
                from: scroll,
                swiped: false,
            };
            CREATIVE.with_borrow_mut(|s| s.finger = Some(finger));
            return;
        }
        // The inventory's tab: its slots and the hotbar's clicked, and dragged from.
        if !tabs.contains(&self.creative_screen.selected) {
            let slot = Self::legacy_slot_at(w, band, p);
            self.slot_down(slot, p, Self::legacy_click);
            return;
        }
        // On an item tab a hotbar slot is chosen for the next item.
        if let Some(slot) = lower_hotbar_slot(w, band, p) {
            if self.selected != slot {
                self.selected = slot;
                self.android_touch_haptic(8);
            }
            return;
        }
        let (grid, _, max, at) = self.creative_layout([w, h, band], tabs.len());
        let page = grid.rows as f32 * SLOT;
        let (_, target) = self.creative_scroll();
        if inside(grid.up, p) {
            self.creative_scroll_to((target - page).max(0.0), false);
            self.android_touch_haptic(8);
        } else if inside(grid.down, p) {
            self.creative_scroll_to((target + page).min(max), false);
            self.android_touch_haptic(8);
        } else if inside(grid.track, p) {
            // Down on the thumb: hold it where it is, without it jumping under the finger.
            let [_, ty, _, th] = grid.track;
            let thumb = dual_menu::SCROLLER[1];
            let middle = ty + thumb / 2.0 + (at / max.max(1.0)) * (th - thumb).max(0.0);
            let grab = if max > 0.0 && (p[1] - middle).abs() <= thumb / 2.0 {
                p[1] - middle
            } else {
                0.0
            };
            CREATIVE.with_borrow_mut(|s| s.finger = Some(Finger::Track { grab }));
            self.creative_track(&grid, max, p[1] - grab);
        } else if inside(Self::creative_grid_area(&grid), p) {
            let finger = Finger::Grid {
                y: p[1],
                from: at,
                swiped: false,
            };
            CREATIVE.with_borrow_mut(|s| s.finger = Some(finger));
        }
    }

    /// A finger moving: the scrollbar held, or the grid swiped (by the pixel).
    fn creative_drag(&mut self, dims: [f32; 3], tabs: usize, p: [f32; 2]) {
        CREATIVE.with_borrow_mut(|s| s.at = p);
        Self::slot_move(p);
        let Some(finger) = CREATIVE.with_borrow(|s| s.finger) else {
            return;
        };
        if let Finger::Tabs { x, from, swiped } = finger {
            let dx = p[0] - x;
            let swiped = swiped || dx.abs() > 4.0;
            let max = dual_menu::tabs_scroll_max(dims[0], dims[1], tabs);
            CREATIVE.with_borrow_mut(|s| {
                s.finger = Some(Finger::Tabs { x, from, swiped });
                if swiped {
                    s.tab_scroll = (from - dx).clamp(0.0, max);
                }
            });
            return;
        }
        let (grid, _, max, _) = self.creative_layout(dims, tabs);
        match finger {
            Finger::Tabs { .. } => {}
            Finger::Track { grab } => self.creative_track(&grid, max, p[1] - grab),
            Finger::Grid { y, from, swiped } => {
                let dy = p[1] - y;
                let swiped = swiped || dy.abs() > 4.0;
                CREATIVE.with_borrow_mut(|s| s.finger = Some(Finger::Grid { y, from, swiped }));
                if swiped {
                    self.creative_scroll_to((from - dy).clamp(0.0, max), true);
                }
            }
        }
    }

    /// A finger lifting: an item tapped (not swiped) is taken.
    fn creative_release(&mut self, dims: [f32; 3], tabs: usize, p: [f32; 2]) {
        let finger = CREATIVE.with_borrow_mut(|s| {
            s.at = p;
            s.finger.take()
        });
        let on_items = self.creative_item_tabs();
        // A tab tapped (not swiped along) is chosen.
        if let Some(Finger::Tabs { swiped, .. }) = finger {
            let scroll = CREATIVE.with_borrow(|s| s.tab_scroll);
            let (cats, _, view) = dual_menu::tabs_scrolled(dims[0], dims[1], tabs, scroll);
            if !swiped
                && inside(view, p)
                && let Some(i) = touch::rect_at(&cats, p)
                && let Some(&tab) = on_items.get(i)
                && self.creative_screen.selected != tab
            {
                self.creative_screen.select(tab);
                self.android_touch_haptic(8);
            }
            return;
        }
        if !on_items.contains(&self.creative_screen.selected) {
            let slot = Self::legacy_slot_at(dims[0], dims[2], p);
            self.slot_up(slot, Self::legacy_click);
            return;
        }
        let Some(Finger::Grid { swiped: false, .. }) = finger else {
            return;
        };
        let (grid, items, _, at) = self.creative_layout(dims, tabs);
        if !inside(Self::creative_grid_area(&grid), p) {
            return;
        }
        let [ox, oy] = grid.origin;
        let (c, r) = ((p[0] - ox) / SLOT, (p[1] - oy + at) / SLOT);
        let index = r as usize * 9 + c as usize;
        if c < 9.0
            && let Some(stack) = items.get(index).filter(|s| s.count > 0)
        {
            self.creative_take(stack.clone(), index);
            self.android_touch_haptic(10);
        }
    }

    /// The 3DS's Take: a full stack of the item in the selected hotbar slot.
    fn creative_take(&mut self, stack: ItemStack, index: usize) {
        if self.spectator() {
            return;
        }
        let max = self.items().get(&stack.item).map_or(64, |i| i.max_stack);
        let full = ItemStack {
            count: max.max(1),
            ..stack.clone()
        };
        if let Some(slot) = self.inventory.get_mut(self.selected) {
            *slot = Some(full);
        }
        // A server hears of it as of any creative slot change (SetCreativeModeSlot).
        if self.remote.is_some() {
            self.send_creative_slots();
        }
        CREATIVE.with_borrow_mut(|s| {
            s.chosen = Some(stack);
            s.index = Some(index);
        });
    }

    fn draw_creative_grid(&mut self, batch: &mut HudBatch, band: f32, tabs: usize) {
        let (w, h) = (batch.width, batch.height);
        let (grid, items, max, at) = self.creative_layout([w, h, band], tabs);
        let page = grid.rows as f32 * SLOT;
        CREATIVE.with_borrow_mut(|s| s.shown = page);
        let title = self.creative_screen.tab().title.clone();
        let title = self.translate(&title, &title);
        let atlas = &self.gui.atlas;
        let [tx, ty, tw, _] = grid.title;
        centered(batch, atlas, &title, [tx, ty, tw], ON_FRAME, false);
        // The rows in sight, the first and last partly, cut to the grid.
        let [ox, oy] = grid.origin;
        let first = (at / SLOT) as usize;
        let shift = at - first as f32 * SLOT;
        let marked = CREATIVE.with_borrow(|s| s.index);
        let start = batch.vertices.len();
        let mut mark = None;
        for r in 0..=grid.rows {
            for c in 0..9 {
                let index = (first + r) * 9 + c;
                if index >= items.len().div_ceil(9) * 9 {
                    break;
                }
                let (x, y) = (ox + c as f32 * SLOT, oy + r as f32 * SLOT - shift);
                slot_frame(batch, &self.gui.atlas, [x, y, SLOT, SLOT]);
                if let Some(stack) = items.get(index).filter(|s| s.count > 0) {
                    let inset = ((SLOT - 16.0) / 2.0).floor();
                    self.draw_item(batch, stack, x + inset, y + inset);
                }
                if marked == Some(index) {
                    mark = Some([x, y, SLOT, SLOT]);
                }
            }
        }
        // After the slots, which would cover its outer line.
        if let Some(rect) = mark {
            highlight(batch, &self.gui.atlas, rect);
        }
        let (right, bottom) = (ox + 9.0 * SLOT + 1.0, oy + page + 1.0);
        batch.clip_since(start, ox - 1.0, oy - 1.0, right, bottom);
        // The scrollbar as vanilla's: its scroller (12 by 15, never stretched) along a sunk
        // track, greyed when the tab doesn't scroll.
        let atlas = &self.gui.atlas;
        let [x, y, bw, bh] = grid.track;
        slot_frame(batch, atlas, [x, y, bw, bh]);
        let [sw, sh] = dual_menu::SCROLLER;
        let top = if max > 0.0 {
            y + ((bh - sh) * at / max).round()
        } else {
            y
        };
        let sprite = if max > 0.0 {
            "gui/sprites/container/creative_inventory/scroller"
        } else {
            "gui/sprites/container/creative_inventory/scroller_disabled"
        };
        match atlas.sprites.get(sprite).copied() {
            Some(r) => batch.rect_uv(x, top, sw, sh, r, u32::MAX),
            None => panel(batch, atlas, [x, top, sw, sh]),
        }
    }

    /// The window while the creative screen is open: the chosen item large (spun by a finger
    /// on the window), and along the bottom "Item Selection", its name and the controls.
    pub(super) fn draw_creative_upper(&mut self, batch: &mut HudBatch) {
        let stack = CREATIVE
            .with_borrow(|s| s.chosen.clone())
            .or_else(|| self.carried.clone())
            .or_else(|| self.inventory.get(self.selected).cloned().flatten());
        let (w, h) = (batch.width, batch.height);
        let center = [(w / 2.0).floor(), (h * 0.40).floor()];
        if let Some(stack) = &stack {
            self.draw_showcase(batch, stack, center);
        }
        let name = stack
            .as_ref()
            .map(|s| item_tooltip::name_line(s, &|key| self.pack.names.display_get(key)));
        let hint = self.pad_hint(
            "{south} or tap: Take    {lb}/{rb}: Hotbar    {lt}/{rt}: Tab    {north}: Clear Hotbar",
        );
        self.draw_upper_caption(batch, Some("Item Selection"), name.as_deref(), &hint);
    }
}
