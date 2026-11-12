//! The dual screen's own inventory and crafting (the player's inventory and a crafting table),
//! laid out across the two screens as New 3DS Edition's touch screen, crafting as Legacy
//! Console Edition's.
//!
//! The lower screen: the hotbar, then either the inventory (armor, off-hand, 27 slots) or one
//! Legacy crafting tab's recipe groups in a row, the selected group's other variants in a strip
//! above and below it, over a tab bar (the tabs from the left, the inventory at the right end).
//! The window, over the running world: the player, or the selected recipe (its grid, missing
//! ingredients in red, its result) beside the inventory. Crafting places one set in the open
//! grid (`ServerboundPlaceRecipePacket`) and shift-clicks the result out, so a server crafts it
//! as for its own recipe book; slots are clicked through the menu (`menu_click`).
use super::*;
use crate::second_screen::Touch;
use horizon_client::{hud::HudAtlas, item_tooltip, menu::Input, touch::dual_menu};
use std::collections::HashMap;

/// Each Legacy crafting tab's icon (`dual_menu::LEGACY_TABS`).
const TAB_ICONS: [&str; 7] = [
    "minecraft:oak_planks",
    "minecraft:iron_pickaxe",
    "minecraft:cooked_beef",
    "minecraft:diamond_chestplate",
    "minecraft:piston",
    "minecraft:rail",
    "minecraft:painting",
];

pub(super) const TEXT: u32 = 0xFF40_4040;
/// Text on the frame: vanilla's container text, dark, without a shadow.
pub(super) const ON_FRAME: u32 = TEXT;

/// The inventory tab's slots: the hotbar's size and pitch.
const INV_SLOT: f32 = dual_menu::INVENTORY_SLOT;

/// Where a screen's contents start under the hotbar band `band`: inside the frame.
pub(super) fn content_top(band: f32) -> f32 {
    band + dual_menu::CONTENT
}
const PANEL: u32 = 0xFFC6_C6C6;
const DARK: u32 = 0xFF37_3737;
const SHADE: u32 = 0xFF55_5555;
const SLOT_FACE: u32 = 0xFF8B_8B8B;
/// Missing ingredients and a result that can't be made: Legacy's red.
const MISSING: u32 = 0xFF30_30C8;

#[derive(Default)]
struct Legacy {
    /// None: the inventory tab; Some(i): crafting tab i.
    tab: Option<usize>,
    group: usize,
    variant: usize,
    /// The screen was open last frame: a new one starts on `requested`, else its default.
    open: bool,
    requested: Option<Option<usize>>,
    /// A craft placed in the grid: its result is shift-clicked out once it shows there (the
    /// item, frames left to wait).
    take: Option<(String, u8)>,
    /// The slot last tapped (its name shows on the window) and where, for the carried stack.
    focus: Option<usize>,
    at: [f32; 2],
    /// The controller's slot on the inventory tab (row and column of `PAD_ROWS`), and whether
    /// the controller was used last (its slot then holds the carried stack).
    pad: (usize, usize),
    pad_active: bool,
}

/// The inventory tab's slots as a controller moves over them, top to bottom: the hotbar,
/// the armor and off-hand, the inventory's three rows.
fn pad_rows() -> [Vec<usize>; 5] {
    [
        (0..9).collect(),
        vec![500, 501, 502, 503, 504],
        (9..18).collect(),
        (18..27).collect(),
        (27..36).collect(),
    ]
}

thread_local! {
    static LEGACY: RefCell<Legacy> = RefCell::new(Legacy::default());
    static DRAG: Cell<Drag> = const { Cell::new(Drag::NONE) };
}

/// Held this long without moving, a stack picked up is split instead (a right click).
const LONG_PRESS: std::time::Duration = std::time::Duration::from_millis(400);

/// A stack picked up by a finger coming down on its slot: it follows the finger, and goes
/// down where the finger lifts, or back to its slot off any. Held still, half of it is taken
/// instead.
#[derive(Clone, Copy)]
struct Drag {
    origin: Option<usize>,
    start: [f32; 2],
    moved: bool,
    /// When the finger came down.
    down: Option<std::time::Instant>,
}

impl Drag {
    const NONE: Self = Self {
        origin: None,
        start: [0.0; 2],
        moved: false,
        down: None,
    };
}

/// The next inventory screen opens on this tab (None: the inventory), or the open one turns to
/// it: the panel's inventory and crafting buttons.
pub(super) fn open_on(tab: Option<usize>) {
    LEGACY.with_borrow_mut(|l| {
        if l.open {
            l.tab = tab;
            l.group = 0;
            l.variant = 0;
        } else {
            l.requested = Some(tab);
        }
    });
}

/// The screen isn't shown (closed, or another took its place).
pub(super) fn closed() {
    LEGACY.with_borrow_mut(|l| {
        l.open = false;
        l.take = None;
    });
}

/// Vanilla's container texture the frames and slots are cut from: the resource pack's, so they
/// look as its chests do. Every pack keeps its layout (176 by 222 of a 256 square, scaled for
/// a higher resolution), which vanilla's own chest screen relies on.
const CONTAINER: &str = "gui/container/generic_54";

/// Texels `[sx, sy, sw, sh]` of `CONTAINER` (in its 256-square layout) drawn over `dst`.
fn container_part(batch: &mut HudBatch, atlas: &HudAtlas, src: [f32; 4], dst: [f32; 4]) -> bool {
    let Some(r) = atlas.sprites.get(CONTAINER).copied() else {
        return false;
    };
    let [dx, dy, dw, dh] = dst;
    if dw <= 0.0 || dh <= 0.0 {
        return true;
    }
    let [sx, sy, sw, sh] = src;
    let (du, dv) = ((r.u1 - r.u0) / 256.0, (r.v1 - r.v0) / 256.0);
    let mut q = r;
    (q.u0, q.u1) = (r.u0 + du * sx, r.u0 + du * (sx + sw));
    (q.v0, q.v1) = (r.v0 + dv * sy, r.v0 + dv * (sy + sh));
    batch.rect_uv(dx, dy, dw, dh, q, u32::MAX);
    true
}

/// `CONTAINER`'s columns and rows `cols`/`rows` (each its start and size in the texture)
/// nine-sliced over `dst`: the corners kept, the edges and middle stretched, the borders
/// `border` pixels (less on a smaller `dst`). False without the texture.
fn container_slices(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    cols: [[f32; 2]; 3],
    rows: [[f32; 2]; 3],
    border: f32,
    [x, y, w, h]: [f32; 4],
) -> bool {
    if !atlas.sprites.contains_key(CONTAINER) {
        return false;
    }
    let bx = border.min((w / 2.0).floor());
    let by = border.min((h / 2.0).floor());
    let across = [[x, bx], [x + bx, w - 2.0 * bx], [x + w - bx, bx]];
    let down = [[y, by], [y + by, h - 2.0 * by], [y + h - by, by]];
    for (row, [ty, th]) in rows.iter().zip(down) {
        for (col, [tx, tw]) in cols.iter().zip(across) {
            let src = [col[0], row[0], col[1], row[1]];
            container_part(batch, atlas, src, [tx, ty, tw, th]);
        }
    }
    true
}

/// Vanilla's slot, from the pack's chest texture (its first slot, 1-pixel edges): dark top and
/// left edges, light bottom and right ones.
pub(super) fn slot_frame(batch: &mut HudBatch, atlas: &HudAtlas, r: [f32; 4]) {
    let (cols, rows) = (
        [[7.0, 1.0], [8.0, 16.0], [24.0, 1.0]],
        [[17.0, 1.0], [18.0, 16.0], [34.0, 1.0]],
    );
    if container_slices(batch, atlas, cols, rows, 1.0, r) {
        return;
    }
    let [x, y, w, h] = r;
    batch.fill(atlas, x, y, w - 1.0, 1.0, DARK);
    batch.fill(atlas, x, y, 1.0, h - 1.0, DARK);
    batch.fill(atlas, x + 1.0, y + h - 1.0, w - 1.0, 1.0, u32::MAX);
    batch.fill(atlas, x + w - 1.0, y + 1.0, 1.0, h - 1.0, u32::MAX);
    batch.fill(atlas, x + 1.0, y + 1.0, w - 2.0, h - 2.0, SLOT_FACE);
}

/// Vanilla's container panel, from the pack's chest texture: its rounded 4-pixel border (the
/// top from its top edge, the bottom from its bottom one) around the plain grey under its title.
pub(super) fn panel(batch: &mut HudBatch, atlas: &HudAtlas, r: [f32; 4]) {
    let (cols, rows) = (
        [[0.0, 4.0], [4.0, 168.0], [172.0, 4.0]],
        [[0.0, 4.0], [4.0, 12.0], [218.0, 4.0]],
    );
    if container_slices(batch, atlas, cols, rows, 4.0, r) {
        return;
    }
    let [x, y, w, h] = r;
    batch.fill(atlas, x, y, w, h, PANEL);
    batch.fill(atlas, x, y, w - 2.0, 2.0, u32::MAX);
    batch.fill(atlas, x, y, 2.0, h - 2.0, u32::MAX);
    batch.fill(atlas, x + 2.0, y + h - 2.0, w - 2.0, 2.0, SHADE);
    batch.fill(atlas, x + w - 2.0, y + 2.0, 2.0, h - 2.0, SHADE);
}

pub(super) fn outline(batch: &mut HudBatch, atlas: &HudAtlas, r: [f32; 4], color: u32) {
    let [x, y, w, h] = r;
    batch.fill(atlas, x - 1.0, y - 1.0, w + 2.0, 1.0, color);
    batch.fill(atlas, x - 1.0, y + h, w + 2.0, 1.0, color);
    batch.fill(atlas, x - 1.0, y, 1.0, h, color);
    batch.fill(atlas, x + w, y, 1.0, h, color);
}

/// A triangle in `r` pointing up, down, left or right.
pub(super) fn arrow(batch: &mut HudBatch, atlas: &HudAtlas, r: [f32; 4], dir: u8, color: u32) {
    let [x, y, w, h] = r;
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    let shape = match dir {
        b'^' => [[cx, y], [x + w, y + h], [x, y + h], [cx, y]],
        b'v' => [[x, y], [x + w, y], [cx, y + h], [x, y]],
        b'<' => [[x, cy], [x + w, y], [x + w, y + h], [x, cy]],
        _ => [[x, y], [x + w, cy], [x, y + h], [x, y]],
    };
    batch.fill_quad(atlas, shape, color);
}

/// The shoulders step the selected hotbar slot; true when `button` was one.
pub(super) fn hotbar_shoulder(
    game: &mut Game<'_>,
    button: horizon_client::gamepad::Button,
) -> bool {
    use horizon_client::gamepad::Button as B;
    let step = match button {
        B::LeftBumper => 8,
        B::RightBumper => 1,
        _ => return false,
    };
    if !game.spectator() {
        game.selected = (game.selected + step) % 9;
    }
    true
}

/// Two-pixel white highlight around a slot, the lower screen's focus colour (vanilla's
/// highlighted button's): drawn after the slots beside it, which would cover its outer line.
pub(super) fn highlight(batch: &mut HudBatch, atlas: &HudAtlas, [x, y, w, h]: [f32; 4]) {
    outline(batch, atlas, [x, y, w, h], u32::MAX);
    outline(batch, atlas, [x + 1.0, y + 1.0, w - 2.0, h - 2.0], u32::MAX);
    batch.fill(atlas, x + 1.0, y + 1.0, w - 2.0, h - 2.0, 0x50FF_FFFF);
}

pub(super) fn inside([x, y, w, h]: [f32; 4], p: [f32; 2]) -> bool {
    p[0] >= x && p[1] >= y && p[0] < x + w && p[1] < y + h
}

/// The sprite an empty armor or off-hand slot shows.
fn empty_icon(slot: usize) -> Option<&'static str> {
    Some(match slot {
        500 => "gui/sprites/container/slot/helmet",
        501 => "gui/sprites/container/slot/chestplate",
        502 => "gui/sprites/container/slot/leggings",
        503 => "gui/sprites/container/slot/boots",
        504 => "gui/sprites/container/slot/shield",
        _ => return None,
    })
}

/// Text at `y`, centered across `x..x + w`.
pub(super) fn centered(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    text: &str,
    [x, y, w]: [f32; 3],
    color: u32,
    shadow: bool,
) {
    let tw = HudBatch::text_width(atlas, text);
    batch.text(atlas, text, (x + (w - tw) / 2.0).floor(), y, color, shadow);
}

impl Game<'_> {
    /// A finger coming down on `slot` (None: on none): it is clicked; a stack it picks up
    /// is dragged.
    pub(super) fn slot_down(
        &mut self,
        slot: Option<usize>,
        p: [f32; 2],
        click: impl Fn(&mut Self, usize),
    ) {
        let mut drag = Drag {
            start: p,
            down: Some(std::time::Instant::now()),
            ..Drag::NONE
        };
        if let Some(slot) = slot {
            self.android_touch_haptic(6);
            let empty = self.carried.is_none();
            click(self, slot);
            if empty && self.carried.is_some() {
                drag.origin = Some(slot);
            }
        }
        DRAG.set(drag);
    }

    /// The finger moving: the dragged stack follows it.
    pub(super) fn slot_move(p: [f32; 2]) {
        let mut drag = DRAG.get();
        let [x, y] = drag.start;
        drag.moved |= (p[0] - x).hypot(p[1] - y) > 6.0;
        DRAG.set(drag);
    }

    /// The finger lifting over `slot`: a dragged stack goes down there (what it swapped
    /// with goes to where it came from), or back where it came from off any slot. A tap
    /// (no drag) keeps the stack carried for the next tap.
    pub(super) fn slot_up(&mut self, slot: Option<usize>, click: impl Fn(&mut Self, usize)) {
        let drag = DRAG.replace(Drag::NONE);
        let Some(origin) = drag.origin.filter(|_| drag.moved) else {
            return;
        };
        if self.carried.is_none() {
            return;
        }
        if let Some(slot) = slot.filter(|&s| s != origin) {
            click(self, slot);
        }
        if self.carried.is_some() {
            click(self, origin);
        }
    }

    /// Each frame: a finger held still on the stack it picked up puts it back and takes half
    /// (`right`, a right click), which then stays carried as after a tap.
    pub(super) fn slot_hold(
        &mut self,
        click: impl Fn(&mut Self, usize),
        right: impl Fn(&mut Self, usize),
    ) {
        let drag = DRAG.get();
        let (Some(origin), Some(down)) = (drag.origin, drag.down) else {
            return;
        };
        if drag.moved || down.elapsed() < LONG_PRESS {
            return;
        }
        DRAG.set(Drag::NONE);
        if self.carried.is_some() {
            click(self, origin);
            right(self, origin);
        }
    }

    /// The inventory tab's slot (or the hotbar's) at `p`.
    pub(super) fn legacy_slot_at(w: f32, band: f32, p: [f32; 2]) -> Option<usize> {
        lower_hotbar_slot(w, band, p).or_else(|| {
            dual_menu::inventory(w, content_top(band))
                .into_iter()
                .find(|&(_, [x, y])| inside([x, y, INV_SLOT, INV_SLOT], p))
                .map(|(slot, _)| slot)
        })
    }

    /// One frame of the lower screen while the inventory or a crafting table is open.
    pub(super) fn legacy_frame(&mut self, size: [u32; 2], touches: &[Touch]) {
        // A newly opened screen: on the tab asked for, else crafting at a crafting table.
        let table = self.craft_size == 3;
        LEGACY.with_borrow_mut(|l| {
            if !l.open {
                l.open = true;
                l.tab = l.requested.take().unwrap_or(table.then_some(0));
                (l.group, l.variant, l.take, l.focus) = (0, 0, None, None);
            }
        });
        self.legacy_take();
        // Small enough for the tabs to be square around their icons and the inventory to fit,
        // as the creative screen's.
        let band = touch::dual_panel(0.0, 0.0).band;
        let scale = dual_menu::inventory_scale(size, TAB_ICONS.len(), band);
        let mut batch = lower_batch_at(size, scale);
        let (w, h) = (batch.width, batch.height);
        let band = touch::dual_panel(w, h).band;
        let scale = batch.scale;
        for touch in touches {
            match *touch {
                Touch::Down([x, y]) => self.legacy_tap(w, h, band, [x / scale, y / scale]),
                Touch::Move([x, y]) => {
                    let p = [x / scale, y / scale];
                    LEGACY.with_borrow_mut(|l| l.at = p);
                    Self::slot_move(p);
                }
                Touch::Up([x, y]) => {
                    let p = [x / scale, y / scale];
                    let on_inventory = LEGACY.with_borrow(|l| l.tab.is_none());
                    let slot = if on_inventory {
                        Self::legacy_slot_at(w, band, p)
                    } else {
                        None
                    };
                    self.slot_up(slot, Self::legacy_click);
                }
            }
            if !self.crafting_screen_open() {
                return;
            }
        }
        self.slot_hold(Self::legacy_click, Self::legacy_right_click);
        self.draw_menu_ground(&mut batch, band, TAB_ICONS.len());
        let tab = LEGACY.with_borrow(|l| l.tab);
        match tab {
            None => self.draw_legacy_inventory(&mut batch, band),
            Some(t) => self.draw_legacy_crafting(&mut batch, band, t),
        }
        let icons: Vec<&str> = TAB_ICONS.to_vec();
        self.draw_lower_tabs(&mut batch, &icons, tab, 0.0);
        // The carried stack, where the last tap was.
        if let Some(stack) = self.carried.clone() {
            let [x, y] = LEGACY.with_borrow(|l| l.at);
            self.draw_item(&mut batch, &stack, x - 8.0, y - 8.0);
        }
        self.present_lower(&mut batch, BACKGROUND);
    }

    /// A placed craft's result, shift-clicked into the inventory once the grid shows it (at
    /// once in single player's own menu, a few frames later from a server).
    fn legacy_take(&mut self) {
        let Some((item, left)) = LEGACY.with_borrow(|l| l.take.clone()) else {
            return;
        };
        // The result slot is computed, not stored (`slot_view`).
        let ready = self.slot_view(200).is_some_and(|s| *s.item == *item);
        if ready {
            self.menu_click(Some(200), 0, Input::QuickMove);
        }
        LEGACY.with_borrow_mut(|l| l.take = (!ready && left > 1).then_some((item, left - 1)));
    }

    /// A controller button on the inventory and crafting screen; true when taken. The
    /// triggers change tab, the shoulders the hotbar slot; crafting: left and right choose the
    /// group, up and down its variant, A crafts; the inventory: the pad moves over the slots,
    /// A takes or puts down all, Y half, X moves the stack across.
    pub(super) fn legacy_pad(&mut self, button: horizon_client::gamepad::Button) -> bool {
        use horizon_client::gamepad::Button as B;
        let tab = LEGACY.with_borrow(|l| l.tab);
        if hotbar_shoulder(self, button) {
            return true;
        }
        if matches!(button, B::LeftTrigger | B::RightTrigger) {
            // The crafting tabs, then the inventory's.
            let n = TAB_ICONS.len() + 1;
            let at = tab.unwrap_or(n - 1);
            let next = if button == B::RightTrigger {
                (at + 1) % n
            } else {
                (at + n - 1) % n
            };
            let next = (next < n - 1).then_some(next);
            LEGACY.with_borrow_mut(|l| (l.tab, l.group, l.variant) = (next, 0, 0));
            return true;
        }
        match tab {
            Some(t) => self.legacy_crafting_pad(t, button),
            None => self.legacy_inventory_pad(button),
        }
    }

    fn legacy_crafting_pad(&mut self, t: usize, button: horizon_client::gamepad::Button) -> bool {
        use horizon_client::gamepad::Button as B;
        let groups = self.crafting_groups(t);
        let (group, variant) = Self::legacy_selection(&groups);
        let variants = groups.get(group).map_or(1, Vec::len).max(1);
        let set = |g: usize, v: usize| LEGACY.with_borrow_mut(|l| (l.group, l.variant) = (g, v));
        match button {
            B::DPadLeft => set(group.saturating_sub(1), 0),
            B::DPadRight => set((group + 1).min(groups.len().saturating_sub(1)), 0),
            B::DPadUp => set(group, (variant + variants - 1) % variants),
            B::DPadDown => set(group, (variant + 1) % variants),
            B::South => {
                if let Some(&entry) = groups.get(group).and_then(|g| g.get(variant)) {
                    self.legacy_craft(entry);
                }
            }
            _ => return false,
        }
        true
    }

    pub(super) fn legacy_inventory_pad(&mut self, button: horizon_client::gamepad::Button) -> bool {
        use horizon_client::gamepad::Button as B;
        let rows = pad_rows();
        let (row, col) = LEGACY.with_borrow(|l| l.pad);
        let moved = match button {
            B::DPadUp => Some((row.saturating_sub(1), col)),
            B::DPadDown => Some(((row + 1).min(rows.len() - 1), col)),
            B::DPadLeft => Some((row, col.saturating_sub(1))),
            B::DPadRight => Some((row, col + 1)),
            _ => None,
        };
        if let Some((row, col)) = moved {
            let col = col.min(rows[row].len() - 1);
            LEGACY.with_borrow_mut(|l| (l.pad, l.pad_active) = ((row, col), true));
            return true;
        }
        let slot = rows[row][col.min(rows[row].len() - 1)];
        let (button, input) = match button {
            B::South => (0, Input::Pickup),
            B::North => (1, Input::Pickup),
            B::West => (0, Input::QuickMove),
            _ => return false,
        };
        LEGACY.with_borrow_mut(|l| (l.focus, l.pad_active) = (Some(slot), true));
        self.menu_click(Some(slot), button, input);
        true
    }

    fn legacy_tap(&mut self, w: f32, h: f32, band: f32, p: [f32; 2]) {
        LEGACY.with_borrow_mut(|l| (l.at, l.pad_active) = (p, false));
        let tab = LEGACY.with_borrow(|l| l.tab);
        let (categories, inventory) = dual_menu::tabs(w, h, TAB_ICONS.len());
        let chosen = if inside(inventory, p) {
            Some(None)
        } else {
            touch::rect_at(&categories, p).map(Some)
        };
        if let Some(chosen) = chosen {
            if chosen != tab {
                self.android_touch_haptic(8);
            }
            LEGACY.with_borrow_mut(|l| (l.tab, l.group, l.variant) = (chosen, 0, 0));
            return;
        }
        match tab {
            // The hotbar is the inventory's: its slots are clicked (and dragged from) too.
            None => {
                let slot = Self::legacy_slot_at(w, band, p);
                self.slot_down(slot, p, Self::legacy_click);
            }
            Some(_) => {
                if let Some(slot) = lower_hotbar_slot(w, band, p) {
                    if !self.spectator() {
                        self.selected = slot;
                    }
                    return;
                }
                if let Some(t) = tab {
                    self.legacy_crafting_tap(w, band, t, p);
                }
            }
        }
    }

    /// A tap on a slot: a left click (`Pickup`), taking or putting down the stack.
    pub(super) fn legacy_click(&mut self, slot: usize) {
        LEGACY.with_borrow_mut(|l| l.focus = Some(slot));
        self.menu_click(Some(slot), 0, Input::Pickup);
    }

    /// A long press on a slot: a right click, half the stack or one put down.
    pub(super) fn legacy_right_click(&mut self, slot: usize) {
        LEGACY.with_borrow_mut(|l| l.focus = Some(slot));
        self.menu_click(Some(slot), 1, Input::Pickup);
    }

    /// The selected group and variant of `groups`, kept in range.
    fn legacy_selection(groups: &[Vec<(usize, bool)>]) -> (usize, usize) {
        LEGACY.with_borrow_mut(|l| {
            l.group = l.group.min(groups.len().saturating_sub(1));
            let variants = groups.get(l.group).map_or(1, Vec::len);
            l.variant = l.variant.min(variants.saturating_sub(1));
            (l.group, l.variant)
        })
    }

    fn legacy_crafting_tap(&mut self, w: f32, band: f32, t: usize, p: [f32; 2]) {
        let groups = self.crafting_groups(t);
        if groups.is_empty() {
            return;
        }
        let (group, variant) = Self::legacy_selection(&groups);
        let visible = dual_menu::carousel_fits(w).min(groups.len());
        let first = dual_menu::window(group, groups.len(), visible);
        let row = dual_menu::carousel(w, content_top(band), visible, group - first);
        let variants = groups[group].len();
        let set = |g: usize, v: usize| LEGACY.with_borrow_mut(|l| (l.group, l.variant) = (g, v));
        if inside(row.left, p) {
            set(group.saturating_sub(1), 0);
        } else if inside(row.right, p) {
            set((group + 1).min(groups.len() - 1), 0);
        } else if variants > 1 && inside(row.up, p) {
            set(group, (variant + variants - 1) % variants);
        } else if variants > 1 && inside(row.down, p) {
            set(group, (variant + 1) % variants);
        } else if let Some(i) = touch::rect_at(&row.cells, p) {
            // The selected recipe tapped again is crafted, as Legacy Console Edition's A.
            if first + i == group {
                self.legacy_craft(groups[group][variant]);
            } else {
                set(first + i, 0);
            }
        } else {
            return;
        }
        self.android_touch_haptic(8);
    }

    fn legacy_craft(&mut self, (entry, craftable): (usize, bool)) {
        if !craftable {
            return;
        }
        let Some(result) = self.book_entry(entry).map(|e| e.result.item.to_string()) else {
            return;
        };
        self.place_book_recipe(entry);
        LEGACY.with_borrow_mut(|l| l.take = Some((result, 40)));
    }

    pub(super) fn draw_legacy_inventory(&mut self, batch: &mut HudBatch, band: f32) {
        // The controller's slot: outlined, holding the carried stack.
        let (active, (row, col)) = LEGACY.with_borrow(|l| (l.pad_active, l.pad));
        let marked = active.then(|| {
            let slot = pad_rows()[row][col];
            let rect = match dual_menu::inventory(batch.width, content_top(band))
                .into_iter()
                .find(|s| s.0 == slot)
            {
                Some((_, [x, y])) => [x, y, INV_SLOT, INV_SLOT],
                None => lower_hotbar_rect(batch.width, slot),
            };
            let at = [rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0];
            LEGACY.with_borrow_mut(|l| (l.at, l.focus) = (at, Some(slot)));
            rect
        });
        self.draw_legacy_slots(batch, band);
        if let Some(rect) = marked {
            highlight(batch, &self.gui.atlas, rect);
        }
    }

    fn draw_legacy_slots(&mut self, batch: &mut HudBatch, band: f32) {
        // The player in its black box beside the armor, turned a little, as the console
        // editions' inventory shows it.
        let [dx, dy, dw, dh] = dual_menu::inventory_doll(batch.width, content_top(band));
        slot_frame(batch, &self.gui.atlas, [dx, dy, dw, dh]);
        let atlas = &self.gui.atlas;
        batch.fill(atlas, dx + 1.0, dy + 1.0, dw - 2.0, dh - 2.0, 0xFF00_0000);
        let doll = [dx + 1.0, dy + 2.0, dx + dw - 1.0, dy + dh - 2.0];
        self.draw_player_turned(batch, doll, (dh / 2.4).floor(), 0.0625, Some(20.0));
        // Items centred in their 20-pixel slots, as the hotbar's.
        let inset = ((INV_SLOT - 16.0) / 2.0).floor();
        for (slot, [x, y]) in dual_menu::inventory(batch.width, content_top(band)) {
            slot_frame(batch, &self.gui.atlas, [x, y, INV_SLOT, INV_SLOT]);
            let (ix, iy) = (x + inset, y + inset);
            match self.slot_mut(slot).clone() {
                Some(stack) => self.draw_item(batch, &stack, ix, iy),
                None => {
                    if let Some(icon) = empty_icon(slot) {
                        batch.sprite(&self.gui.atlas, icon, ix, iy);
                    }
                }
            }
        }
    }

    /// The tab bar over the sky: `icons`' tabs from the left (`chosen`, Some), moved `scroll`
    /// to the left and cut to their part of the bar, the inventory's at the right end
    /// (`chosen` None).
    pub(super) fn draw_lower_tabs(
        &mut self,
        batch: &mut HudBatch,
        icons: &[&str],
        chosen: Option<usize>,
        scroll: f32,
    ) {
        let (w, h) = (batch.width, batch.height);
        let (categories, inventory, view) = dual_menu::tabs_scrolled(w, h, icons.len(), scroll);
        let start = batch.vertices.len();
        for (i, rect) in categories.into_iter().enumerate() {
            if rect[0] + rect[2] > view[0] && rect[0] < view[0] + view[2] {
                self.lower_tab(batch, rect, chosen == Some(i), icons[i]);
            }
        }
        // The chosen tab rises 3 into the frame's gap.
        let [vx, vy, vw, vh] = view;
        batch.clip_since(start, vx, vy - 3.0, vx + vw, vy + vh);
        self.lower_tab(batch, inventory, chosen.is_none(), "minecraft:chest");
    }

    /// What the inventory, crafting and creative screens stand on: the world's sky (the
    /// panel's), the hotbar over its top, and the frame from under it to the tab bar of
    /// `tabs` tabs.
    pub(super) fn draw_menu_ground(&mut self, batch: &mut HudBatch, band: f32, tabs: usize) {
        let (w, h) = (batch.width, batch.height);
        let sky = [0.0, 0.0, w, h];
        self.draw_panel_sky(batch, sky, [0.0, band, w, h - band], true);
        self.draw_lower_hotbar_over(batch, band, false);
        let (_, _, view) = dual_menu::tabs_scrolled(w, h, tabs, 0.0);
        let frame = dual_menu::content_frame(w, band, view[1] - 2.0);
        super::draw_frame(batch, &self.gui.atlas, frame);
    }

    /// A tab, as Legacy Console Edition's: the chosen one in the panel's grey and raised to
    /// meet it, the others sunk and darker.
    fn lower_tab(&self, batch: &mut HudBatch, [x, y, w, h]: [f32; 4], chosen: bool, icon: &str) {
        let atlas = &self.gui.atlas;
        if chosen {
            panel(batch, atlas, [x, y - 3.0, w, h + 3.0]);
        } else {
            slot_frame(batch, atlas, [x, y, w, h]);
        }
        let stack = ItemStack::new(icon, 1);
        let start = batch.vertices.len();
        self.draw_item(batch, &stack, x + (w - 16.0) / 2.0, y + (h - 16.0) / 2.0);
        // A tab too small for the icon and its frame: the icon shrinks to fit.
        if w < 18.0 {
            let middle = [x + w / 2.0, y + h / 2.0];
            hud::transform_since(batch, start, middle, (w - 2.0) / 16.0, [0.0, 0.0]);
        }
    }

    fn draw_legacy_crafting(&mut self, batch: &mut HudBatch, band: f32, t: usize) {
        let groups = self.crafting_groups(t);
        let w = batch.width;
        let visible = dual_menu::carousel_fits(w).min(groups.len());
        let (group, variant) = Self::legacy_selection(&groups);
        let first = dual_menu::window(group, groups.len(), visible);
        let row = dual_menu::carousel(w, content_top(band), visible, group - first);
        let atlas = &self.gui.atlas;
        let [hx, hy, hw, _] = row.header;
        centered(
            batch,
            atlas,
            dual_menu::LEGACY_TABS[t],
            [hx, hy, hw],
            ON_FRAME,
            false,
        );
        let Some(&(selected, _)) = groups.get(group).and_then(|g| g.get(variant)) else {
            let at = [hx, hy + 30.0, hw];
            centered(batch, atlas, "No recipes yet", at, ON_FRAME, false);
            return;
        };
        // How many of the selected result the player holds, at the right: "3 x [item]".
        if let Some(result) = self.book_entry(selected).map(|e| e.result.clone()) {
            let held: u32 = self
                .inventory
                .iter()
                .flatten()
                .filter(|s| s.item == result.item)
                .map(|s| s.count)
                .sum();
            let text = format!("{held} x");
            let tw = HudBatch::text_width(atlas, &text);
            batch.text(atlas, &text, hx + hw - 18.0 - tw, hy, ON_FRAME, false);
            let icon = ItemStack::new(&result.item, 1);
            self.draw_item(batch, &icon, hx + hw - 16.0, hy - 4.0);
        }
        // The selected group's other variants: Legacy's strip, framed, arrows at its ends.
        let variants = &groups[group];
        if variants.len() > 1 {
            let atlas = &self.gui.atlas;
            let top = row.up[1] - 8.0;
            let strip = [
                row.up[0] - 3.0,
                top,
                row.up[2] + 6.0,
                row.down[1] + 24.0 - top,
            ];
            panel(batch, atlas, strip);
            let (ax, size) = (row.up[0] + row.up[2] / 2.0 - 3.0, 6.0);
            arrow(batch, atlas, [ax, top + 1.0, size, 5.0], b'^', SHADE);
            let bottom = strip[1] + strip[3] - 6.0;
            arrow(batch, atlas, [ax, bottom, size, 5.0], b'v', SHADE);
        }
        for (i, cell) in row.cells.iter().copied().enumerate() {
            let g = first + i;
            let (entry, craftable) = groups[g][if g == group { variant } else { 0 }];
            self.legacy_recipe_cell(batch, cell, entry, craftable);
            if g == group {
                outline(batch, &self.gui.atlas, cell, u32::MAX);
            }
        }
        if variants.len() > 1 {
            let n = variants.len();
            let up = variants[(variant + n - 1) % n];
            let down = variants[(variant + 1) % n];
            self.legacy_recipe_cell(batch, row.up, up.0, up.1);
            self.legacy_recipe_cell(batch, row.down, down.0, down.1);
        }
        // Arrows where groups are out of sight.
        let atlas = &self.gui.atlas;
        if first > 0 {
            arrow(batch, atlas, row.left, b'<', ON_FRAME);
        }
        if first + visible < groups.len() {
            arrow(batch, atlas, row.right, b'>', ON_FRAME);
        }
    }

    /// A recipe's result in a slot; dimmed when the ingredients aren't held.
    fn legacy_recipe_cell(&self, batch: &mut HudBatch, r: [f32; 4], entry: usize, craftable: bool) {
        slot_frame(batch, &self.gui.atlas, r);
        let Some(result) = self.book_entry(entry).map(|e| e.result.clone()) else {
            return;
        };
        let [x, y, w, h] = r;
        self.draw_item(batch, &result, x + (w - 16.0) / 2.0, y + (h - 16.0) / 2.0);
        if !craftable {
            batch.fill(
                &self.gui.atlas,
                x + 1.0,
                y + 1.0,
                w - 2.0,
                h - 2.0,
                0xAA8B_8B8B,
            );
        }
    }

    /// Over the window's running world while the dual screen's inventory, crafting or creative
    /// screen is open.
    pub(in crate::game) fn draw_legacy_upper(&mut self, batch: &mut HudBatch) {
        if !self.screen_on_second_screen() {
            return;
        }
        // The world under the inventory and creative screens (blurred, by `menu_blur`): clear
        // at the top, darkening to near black at the bottom, where the text is.
        if matches!(self.screen, Screen::Inventory | Screen::Creative) {
            let (w, h) = (batch.width, batch.height);
            let top = (h * 0.15).floor();
            batch.fill_gradient(&self.gui.atlas, (0.0, top, w, h - top), 0, 0xE000_0000);
        }
        if self.screen == Screen::Creative {
            if super::VANILLA_CREATIVE {
                return self.draw_creative_container_upper(batch);
            }
            return self.draw_creative_upper(batch);
        }
        if self.screen == Screen::Paused && !super::VANILLA_PAUSE {
            return self.draw_pause_upper(batch);
        }
        if self.screen == Screen::Inventory && !self.crafting_screen_open() {
            return self.draw_container_upper(batch);
        }
        if self.screen == Screen::Dead {
            return self.draw_death_upper(batch);
        }
        if self.screen != Screen::Inventory {
            return self.draw_screen_header(batch);
        }
        if !self.crafting_screen_open() {
            return;
        }
        match LEGACY.with_borrow(|l| l.tab) {
            None => self.draw_upper_inventory(batch),
            Some(t) => self.draw_upper_recipe(batch, t),
        }
    }

    /// The player over the world, and along the bottom the tapped item's name under
    /// "Inventory" (or that alone) and the controls.
    fn draw_upper_inventory(&mut self, batch: &mut HudBatch) {
        let (w, h) = (batch.width, batch.height);
        // Its middle where the other screens' item is.
        self.draw_showcase_player(batch, [(w / 2.0).floor(), (h * 0.40).floor()]);
        let title = self.translate("container.inventory", "Inventory");
        let focused = LEGACY.with_borrow(|l| l.focus);
        let stack = self
            .carried
            .clone()
            .or_else(|| focused.and_then(|slot| self.slot_mut(slot).clone()));
        let name =
            stack.map(|s| item_tooltip::name_line(&s, &|key| self.pack.names.display_get(key)));
        let (heading, name) = match &name {
            Some(name) => (Some(title.as_str()), name.as_str()),
            None => (None, title.as_str()),
        };
        let hint = self.pad_hint(
            "{south} Take All    {north} Take Half    {west} Quick Move    {lt}/{rt} Tab    {lb}/{rb} Hotbar",
        );
        self.draw_upper_caption(batch, heading, Some(name), &hint);
    }

    /// Legacy Console Edition's crafting panels, on the window: the selected recipe (its name,
    /// grid with the missing ingredients in red, arrow and result, red when it can't be made)
    /// beside the inventory.
    fn draw_upper_recipe(&mut self, batch: &mut HudBatch, t: usize) {
        let groups = self.crafting_groups(t);
        let (group, variant) = Self::legacy_selection(&groups);
        let (w, h) = (batch.width, batch.height);
        let n = self.craft_size.max(2);
        let grid = n as f32 * 18.0;
        let (rw, iw, gap) = (150.0, 9.0 * 18.0 + 12.0, 6.0);
        // The inventory panel when there is room for both.
        let both = w >= rw + gap + iw + 8.0;
        let ph = 14.0 + 4.0 * 18.0 + 4.0 + 8.0;
        let total = if both { rw + gap + iw } else { rw };
        let (x, y) = (((w - total) / 2.0).floor(), ((h - ph) / 2.0).floor());
        panel(batch, &self.gui.atlas, [x, y, rw, ph]);
        if both {
            self.draw_upper_inventory_panel(batch, [x + rw + gap, y, iw, ph]);
        }
        let hint =
            self.pad_hint("{south} or tap again: Craft    {lt}/{rt}: Tab    {lb}/{rb}: Hotbar");
        let at = [x, y + ph + 4.0, total];
        centered(batch, &self.gui.atlas, &hint, at, u32::MAX, true);
        let Some(&(entry, craftable)) = groups.get(group).and_then(|g| g.get(variant)) else {
            let atlas = &self.gui.atlas;
            let text = "No recipes in this tab yet";
            centered(batch, atlas, text, [x, y + ph / 2.0 - 4.0, rw], TEXT, false);
            return;
        };
        let Some(recipe) = self.book_entry(entry).cloned() else {
            return;
        };
        let atlas = &self.gui.atlas;
        let name = item_tooltip::name_line(&recipe.result, &|key| self.pack.names.display_get(key));
        centered(batch, atlas, &name, [x, y + 5.0, rw], TEXT, false);
        // What is held of each item, used up cell by cell as crafting would.
        let mut held: HashMap<String, u32> = HashMap::new();
        for s in self.inventory.iter().flatten() {
            *held.entry(s.item.to_string()).or_default() += s.count;
        }
        let choices: Vec<Vec<String>> = recipe
            .requirements()
            .iter()
            .map(|i| self.book_items_of(i))
            .collect();
        let (gx, gy) = (
            x + 10.0,
            y + 16.0 + ((ph - 16.0 - grid) / 2.0).floor() - 2.0,
        );
        for cell in 0..n * n {
            let (cx, cy) = (gx + (cell % n) as f32 * 18.0, gy + (cell / n) as f32 * 18.0);
            slot_frame(batch, &self.gui.atlas, [cx, cy, 18.0, 18.0]);
        }
        for (ingredient, cell) in recipe.grid_cells(n, n) {
            let Some(k) = ingredient else {
                continue;
            };
            let options = &choices[k];
            let (cx, cy) = (gx + (cell % n) as f32 * 18.0, gy + (cell / n) as f32 * 18.0);
            let have = options
                .iter()
                .find(|item| held.get(*item).is_some_and(|&c| c > 0))
                .cloned();
            match &have {
                Some(item) => {
                    if let Some(c) = held.get_mut(item) {
                        *c -= 1;
                    }
                }
                None => batch.fill(&self.gui.atlas, cx + 1.0, cy + 1.0, 16.0, 16.0, MISSING),
            }
            if let Some(item) = have.or_else(|| options.first().cloned()) {
                self.draw_item(batch, &ItemStack::new(&item, 1), cx + 1.0, cy + 1.0);
            }
        }
        // The arrow, then the result in a larger slot.
        let atlas = &self.gui.atlas;
        let (ax, ay) = (gx + grid + 6.0, gy + grid / 2.0);
        batch.fill(atlas, ax, ay - 2.0, 12.0, 4.0, SHADE);
        arrow(batch, atlas, [ax + 12.0, ay - 6.0, 7.0, 12.0], b'>', SHADE);
        let (rx, ry) = (ax + 26.0, ay - 13.0);
        slot_frame(batch, atlas, [rx, ry, 26.0, 26.0]);
        if !craftable {
            batch.fill(atlas, rx + 1.0, ry + 1.0, 24.0, 24.0, MISSING);
        }
        self.draw_item(batch, &recipe.result, rx + 5.0, ry + 5.0);
    }

    /// Legacy's "Inventory" panel beside the recipe: the 27 slots, then the hotbar.
    fn draw_upper_inventory_panel(&mut self, batch: &mut HudBatch, r: [f32; 4]) {
        let [x, y, w, _] = r;
        let atlas = &self.gui.atlas;
        panel(batch, atlas, r);
        let title = self.translate("container.inventory", "Inventory");
        centered(batch, atlas, &title, [x, y + 5.0, w], TEXT, false);
        for i in 0..36 {
            let (col, row) = (i % 9, i / 9);
            // The hotbar under the inventory, 4 lower.
            let (slot, gap) = if row < 3 { (9 + i, 0.0) } else { (col, 4.0) };
            let (sx, sy) = (
                x + 6.0 + col as f32 * 18.0,
                y + 16.0 + row as f32 * 18.0 + gap,
            );
            slot_frame(batch, &self.gui.atlas, [sx, sy, 18.0, 18.0]);
            if let Some(stack) = self.inventory.get(slot).cloned().flatten() {
                self.draw_item(batch, &stack, sx + 1.0, sy + 1.0);
            }
        }
    }
}
