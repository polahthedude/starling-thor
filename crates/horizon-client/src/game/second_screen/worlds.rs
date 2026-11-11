//! The world list on the dual screen, after New 3DS Edition's "Worlds": a header bar (Delete at
//! its right once a world is chosen), a row per world (its icon, name and game mode, its
//! version at the right) with an edit button beside it, a row to create a world, and a
//! scrollbar; a swipe scrolls. A world tapped once is chosen, again is played. The world
//! list's own buttons do the work (`press_title_button`), so playing, editing, creating and
//! deleting are vanilla's, confirmations included.
use super::menu::{centered, inside, outline, panel};
use super::*;
use crate::second_screen::Touch;
use horizon_client::{
    title::{Mode, WorldEntry},
    touch::dual_menu,
};

/// `Title::buttons`' ids on the world list.
const PLAY: u8 = 10;
const CREATE: u8 = 11;
const EDIT: u8 = 13;
const DELETE: u8 = 14;

#[derive(Default)]
struct List {
    scroll: f32,
    /// A finger down on the list: where it came down and the scroll then.
    finger: Option<(f32, f32)>,
    /// It moved far enough to be a swipe, not a tap.
    swiped: bool,
    /// The controller is on Create New (not a world).
    create: bool,
    /// The lower screen's height, for the controller's scrolling.
    height: f32,
}

thread_local! {
    static LIST: RefCell<List> = RefCell::new(List::default());
}

impl Game<'_> {
    pub(super) fn worlds_on_second_screen(&self) -> bool {
        !matches!(self.screen, Screen::Options | Screen::Mod)
            && self
                .title
                .as_ref()
                .is_some_and(|t| matches!(t.mode, Mode::Select { .. }))
    }

    fn world_list_now(&self) -> (Vec<WorldEntry>, Option<usize>) {
        match self.title.as_ref().map(|t| &t.mode) {
            Some(Mode::Select { worlds, selected }) => (worlds.clone(), *selected),
            _ => (Vec::new(), None),
        }
    }

    fn select_world(&mut self, i: usize) {
        if let Some(title) = self.title.as_mut()
            && let Mode::Select { selected, .. } = &mut title.mode
        {
            *selected = Some(i);
        }
        LIST.with_borrow_mut(|l| l.create = false);
    }

    pub(super) fn worlds_frame(
        &mut self,
        events: &dyn EventLoopCtl,
        size: [u32; 2],
        touches: &[Touch],
    ) {
        let mut batch = lower_batch(size);
        let (w, h, scale) = (batch.width, batch.height, batch.scale);
        let (worlds, selected) = self.world_list_now();
        let count = worlds.len() + 1;
        let max = dual_menu::world_list_scroll_max(h, count);
        LIST.with_borrow_mut(|l| {
            l.height = h;
            l.scroll = l.scroll.clamp(0.0, max);
        });
        for touch in touches {
            match *touch {
                Touch::Down([_, y]) => LIST.with_borrow_mut(|l| {
                    l.finger = Some((y / scale, l.scroll));
                    l.swiped = false;
                }),
                Touch::Move([_, y]) => LIST.with_borrow_mut(|l| {
                    if let Some((from, scroll)) = l.finger {
                        let dy = y / scale - from;
                        l.swiped |= dy.abs() > 4.0;
                        if l.swiped {
                            l.scroll = (scroll - dy).clamp(0.0, max);
                        }
                    }
                }),
                Touch::Up([x, y]) => {
                    let tap = LIST.with_borrow_mut(|l| !std::mem::take(&mut l.swiped));
                    LIST.with_borrow_mut(|l| l.finger = None);
                    if tap {
                        self.worlds_tap(events, [x / scale, y / scale], &worlds, selected, [w, h]);
                        if !self.worlds_on_second_screen() {
                            return;
                        }
                    }
                }
            }
        }
        let (worlds, selected) = self.world_list_now();
        let scroll = LIST.with_borrow(|l| l.scroll);
        let create_focused = LIST.with_borrow(|l| l.create);
        let layout = dual_menu::world_list(w, h, worlds.len() + 1, scroll);
        // The header: the grass block, the title, and Delete for the chosen world.
        let atlas = &self.gui.atlas;
        panel(&mut batch, atlas, layout.header);
        let title = self.translate("selectWorld.title", "Select World");
        let atlas = &self.gui.atlas;
        centered(
            &mut batch,
            atlas,
            &title,
            [0.0, 5.0, w],
            super::menu::TEXT,
            false,
        );
        self.draw_item(
            &mut batch,
            &ItemStack::new("minecraft:grass_block", 1),
            2.0,
            1.0,
        );
        if selected.is_some() {
            let [x, y, bw, bh] = layout.delete;
            let atlas = &self.gui.atlas;
            batch.fill(atlas, x, y, bw, bh, 0xFF20_2070);
            batch.fill(atlas, x + 1.0, y + 1.0, bw - 2.0, bh - 2.0, 0xFF40_40C8);
            let label = self.translate("selectWorld.delete", "Delete");
            let atlas = &self.gui.atlas;
            centered(&mut batch, atlas, &label, [x, y + 3.0, bw], u32::MAX, true);
        }
        // The rows, clipped to the list.
        let start = batch.vertices.len();
        for (i, row, edit) in layout.rows.iter().copied() {
            let [x, y, rw, rh] = row;
            let atlas = &self.gui.atlas;
            batch.fill(atlas, x, y, rw, rh, 0xFF8B_8B8B);
            batch.fill(atlas, x + 1.0, y + 1.0, rw - 2.0, rh - 2.0, 0xFF3C_3C3C);
            let chosen = match worlds.get(i) {
                Some(world) => {
                    if let Some(title) = &self.title {
                        let atlas = &self.gui.atlas;
                        title.draw_world_icon(
                            atlas,
                            &mut batch,
                            world,
                            i % 32,
                            x + 4.0,
                            y + 4.0,
                            24.0,
                        );
                    }
                    let mode = match (world.hardcore, world.game_mode) {
                        (true, _) => self.translate("gameMode.hardcore", "Hardcore Mode"),
                        (_, 1) => self.translate("gameMode.creative", "Creative Mode"),
                        (_, 2) => self.translate("gameMode.adventure", "Adventure Mode"),
                        (_, 3) => self.translate("gameMode.spectator", "Spectator Mode"),
                        _ => self.translate("gameMode.survival", "Survival Mode"),
                    };
                    let atlas = &self.gui.atlas;
                    batch.text(atlas, &world.name, x + 32.0, y + 6.0, u32::MAX, true);
                    batch.text(atlas, &mode, x + 32.0, y + 19.0, 0xFFA0_A0A0, false);
                    let vw = HudBatch::text_width(atlas, &world.version);
                    batch.text(
                        atlas,
                        &world.version,
                        x + rw - 4.0 - vw,
                        y + 19.0,
                        0xFF80_8080,
                        false,
                    );
                    let label = self.translate("selectWorld.edit", "Edit");
                    self.lower_list_button(&mut batch, edit, &label, world_editable(world));
                    selected == Some(i) && !create_focused
                }
                None => {
                    let atlas = &self.gui.atlas;
                    batch.fill(atlas, x + 4.0, y + 4.0, 24.0, 24.0, 0xFF2A_2A2A);
                    let label = self.translate("selectWorld.create", "Create New World");
                    let atlas = &self.gui.atlas;
                    batch.text(atlas, &label, x + 32.0, y + 12.0, u32::MAX, true);
                    create_focused
                }
            };
            if chosen {
                outline(&mut batch, &self.gui.atlas, row, 0xFF3F_E0FF);
            }
        }
        let [lx, ly, lw, lh] = layout.list;
        batch.clip_since(start, lx, ly, lx + lw, ly + lh);
        // The scrollbar.
        let [tx, ty, tw, th] = layout.track;
        let atlas = &self.gui.atlas;
        batch.fill(atlas, tx, ty, tw, th, 0xFF37_3737);
        let thumb = if max > 0.0 {
            (th * th / (th + max)).max(10.0)
        } else {
            th
        };
        let at = if max > 0.0 {
            ty + (th - thumb) * scroll / max
        } else {
            ty
        };
        panel(&mut batch, atlas, [tx, at, tw, thumb]);
        self.present_lower(&batch, WORLDS_BACKGROUND);
    }

    /// A vanilla button `rect`, `label` centered (greyed when inactive).
    fn lower_list_button(&self, batch: &mut HudBatch, rect: [f32; 4], label: &str, active: bool) {
        let atlas = &self.gui.atlas;
        let [x, y, w, h] = rect;
        let sprite = if active {
            "gui/sprites/widget/button"
        } else {
            "gui/sprites/widget/button_disabled"
        };
        ui::nine_slice(batch, atlas, sprite, x, y, w, h, u32::MAX);
        let color = if active { u32::MAX } else { 0xFFA0_A0A0 };
        centered(
            batch,
            atlas,
            label,
            [x, y + ((h - 8.0) / 2.0).floor(), w],
            color,
            true,
        );
    }

    fn worlds_tap(
        &mut self,
        events: &dyn EventLoopCtl,
        p: [f32; 2],
        worlds: &[WorldEntry],
        selected: Option<usize>,
        [w, h]: [f32; 2],
    ) {
        let scroll = LIST.with_borrow(|l| l.scroll);
        let layout = dual_menu::world_list(w, h, worlds.len() + 1, scroll);
        if inside(layout.delete, p) {
            if selected.is_some() {
                self.press_title_button(events, DELETE);
            }
            return;
        }
        if !inside(layout.list, p) {
            return;
        }
        for (i, row, edit) in layout.rows {
            if i == worlds.len() {
                if inside(row, p) {
                    self.press_title_button(events, CREATE);
                }
                continue;
            }
            if inside(edit, p) {
                self.select_world(i);
                self.press_title_button(events, EDIT);
                return;
            }
            if inside(row, p) {
                // Chosen, then played, as the 3DS's.
                if selected == Some(i) {
                    self.press_title_button(events, PLAY);
                } else {
                    self.select_world(i);
                }
                return;
            }
        }
    }

    /// A controller button on the world list; true when taken: up and down choose a world
    /// (or Create New), A plays it (or creates one), Y edits it, X deletes it.
    pub(super) fn worlds_pad(
        &mut self,
        events: &dyn EventLoopCtl,
        button: horizon_client::gamepad::Button,
    ) -> bool {
        use horizon_client::gamepad::Button as B;
        let (worlds, selected) = self.world_list_now();
        let create = LIST.with_borrow(|l| l.create) || worlds.is_empty();
        let at = if create {
            worlds.len()
        } else {
            selected.unwrap_or(0)
        };
        let to = match button {
            B::DPadUp => Some(at.saturating_sub(1)),
            B::DPadDown => Some((at + 1).min(worlds.len())),
            _ => None,
        };
        if let Some(to) = to {
            if to == worlds.len() {
                LIST.with_borrow_mut(|l| l.create = true);
            } else {
                self.select_world(to);
            }
            // Scrolled to keep it in sight.
            LIST.with_borrow_mut(|l| {
                let top = to as f32 * dual_menu::WORLD_ROW;
                let shown = (l.height - 30.0).max(dual_menu::WORLD_ROW);
                if top < l.scroll {
                    l.scroll = top;
                } else if top + dual_menu::WORLD_ROW > l.scroll + shown {
                    l.scroll = top + dual_menu::WORLD_ROW - shown;
                }
            });
            return true;
        }
        let id = match button {
            B::South if create => CREATE,
            B::South => PLAY,
            B::North if !create => EDIT,
            B::West if !create => DELETE,
            _ => return false,
        };
        if !create && selected.is_none() {
            self.select_world(at);
        }
        self.press_title_button(events, id);
        true
    }
}

/// The 3DS's dark grey around the list.
const WORLDS_BACKGROUND: wgpu::Color = wgpu::Color {
    r: 85.0 / 255.0,
    g: 85.0 / 255.0,
    b: 85.0 / 255.0,
    a: 1.0,
};

/// `LevelSummary.canEdit` as the Edit button shows it.
fn world_editable(world: &WorldEntry) -> bool {
    !world.corrupted && !world.locked && world.incompatible.is_none() && !world.file_fixing
}
