//! Create New World and Edit World on the dual screen, after New 3DS Edition's: a header with
//! the screen's name and its green Create (or Save) button, then one scrolling column of
//! labelled fields: text boxes (a tap brings the system keyboard up), choices that cycle on a
//! tap, switches, and buttons. Every choice is the vanilla screen's own button pressed
//! (`press_title_button`, on the tab vanilla has it on), so its rules (Hardcore's locked
//! difficulty, the flat world's presets...) are vanilla's.
use super::menu::{arrow, centered, inside, outline, panel};
use super::*;
use crate::second_screen::Touch;
use horizon_client::title::Mode;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// A text box; its field (`Mode::Create`'s focus, or Edit's name).
    Text(u8),
    Cycle,
    Switch(bool),
    Button,
}

struct Row {
    label: String,
    value: String,
    kind: Kind,
    /// The vanilla button's id, and the create screen's tab it is on.
    id: u8,
    tab: usize,
    enabled: bool,
}

impl Row {
    fn height(&self) -> f32 {
        if self.kind == Kind::Button {
            22.0
        } else {
            30.0
        }
    }
}

/// The header's button: its label, id and whether it can be pressed.
type HeaderButton = (String, u8, bool);

#[derive(Default)]
struct Form {
    scroll: f32,
    finger: Option<(f32, f32)>,
    swiped: bool,
    /// The controller's row.
    pad: usize,
    height: f32,
    /// A choice's dropdown is open: its row's id and the value the controller is on.
    open: Option<(u8, usize)>,
}

/// A dropdown item's height.
const ITEM: f32 = 14.0;

/// The dropdown's items under `control` (above it when they would leave the screen, `height`
/// tall).
fn dropdown([cx, cy, cw, ch]: [f32; 4], n: usize, height: f32) -> Vec<[f32; 4]> {
    let list = n as f32 * ITEM;
    let top = if cy + ch + list > height - 2.0 {
        (cy - list).max(HEADER)
    } else {
        cy + ch
    };
    (0..n)
        .map(|i| [cx, top + i as f32 * ITEM, cw, ITEM])
        .collect()
}

thread_local! {
    static FORM: RefCell<Form> = RefCell::new(Form::default());
}

const HEADER: f32 = 34.0;
const GREEN: u32 = 0xFF40_A050;

/// The 3DS's dark grey under the form.
const FORM_BACKGROUND: wgpu::Color = wgpu::Color {
    r: 85.0 / 255.0,
    g: 85.0 / 255.0,
    b: 85.0 / 255.0,
    a: 1.0,
};

/// A row: its label's language key and English, the value shown, its kind, the vanilla
/// button's id and tab, and whether it can be used.
type Spec<'a> = (&'a str, &'a str, String, Kind, u8, usize, bool);

impl Game<'_> {
    pub(super) fn form_on_second_screen(&self) -> bool {
        !super::VANILLA_WORLDS
            && !matches!(self.screen, Screen::Options | Screen::Mod)
            && self.title.as_ref().is_some_and(|t| {
                matches!(t.mode, Mode::Create { .. } | Mode::Edit { .. }) && !t.overlay_open()
            })
    }

    /// A choice's values in the order its button steps through them, and the one shown.
    fn form_values(&self, id: u8) -> Option<(Vec<String>, usize)> {
        let Some(Mode::Create {
            creative, options, ..
        }) = self.title.as_ref().map(|t| &t.mode)
        else {
            return None;
        };
        let t = |key: &str, plain: &str| self.translate(key, plain);
        Some(match id {
            // Survival, Hardcore, Creative: `WorldOptions::cycle`'s order.
            20 => {
                let modes = [
                    t("selectWorld.gameMode.survival", "Survival"),
                    t("selectWorld.gameMode.hardcore", "Hardcore"),
                    t("selectWorld.gameMode.creative", "Creative"),
                ];
                let at = match (*creative, options.hardcore) {
                    (true, _) => 2,
                    (false, true) => 1,
                    _ => 0,
                };
                (modes.to_vec(), at)
            }
            23 => {
                // `create_world::DIFFICULTIES`.
                let levels = ["peaceful", "easy", "normal", "hard"];
                let names = levels
                    .iter()
                    .map(|l| t(&format!("options.difficulty.{l}"), l))
                    .collect();
                (names, options.difficulty().min(levels.len() - 1))
            }
            // From the one shown, round to it.
            25 => {
                let mut names = Vec::new();
                let mut preset = options.world_type;
                loop {
                    names.push(t(preset.label(), preset.label()));
                    preset = preset.next();
                    if preset == options.world_type || names.len() >= 16 {
                        break;
                    }
                }
                (names, 0)
            }
            _ => return None,
        })
    }

    /// `row`'s value `to` chosen: its button pressed round to it.
    fn form_choose(&mut self, events: &dyn EventLoopCtl, row: &Row, to: usize) {
        let Some((values, at)) = self.form_values(row.id) else {
            return;
        };
        let n = values.len();
        for _ in 0..(to + n - at) % n {
            self.form_press(events, row);
        }
    }

    fn form_rows(&self, specs: Vec<Spec<'_>>) -> Vec<Row> {
        specs
            .into_iter()
            .map(|(key, plain, value, kind, id, tab, enabled)| Row {
                label: self.translate(key, plain),
                value,
                kind,
                id,
                tab,
                enabled,
            })
            .collect()
    }

    /// The form's title, header button and rows, from the open screen's state.
    fn form_now(&self) -> (String, HeaderButton, Vec<Row>) {
        let t = |key: &str, plain: &str| self.translate(key, plain);
        let none = String::new;
        match self.title.as_ref().map(|t| &t.mode) {
            Some(Mode::Create {
                name,
                seed,
                creative,
                options,
                ..
            }) => {
                let mode = match (*creative, options.hardcore) {
                    (true, _) => t("selectWorld.gameMode.creative", "Creative"),
                    (false, true) => t("selectWorld.gameMode.hardcore", "Hardcore"),
                    _ => t("selectWorld.gameMode.survival", "Survival"),
                };
                let level = ["peaceful", "easy", "normal", "hard"][options.difficulty().min(3)];
                let difficulty = t(&format!("options.difficulty.{level}"), level);
                let flat = options.world_type == horizon_client::save::WorldPreset::Flat;
                let world_type = t(options.world_type.label(), "Default");
                let soft = !options.hardcore;
                let commands = Kind::Switch(options.commands(*creative));
                let structures = Kind::Switch(options.structures);
                let bonus = Kind::Switch(options.bonus_chest());
                #[rustfmt::skip]
                let rows = self.form_rows(vec![
                    ("selectWorld.enterName", "World Name", name.clone(), Kind::Text(0), 0, 0, true),
                    ("selectWorld.gameMode", "Game Mode", mode, Kind::Cycle, 20, 0, true),
                    ("options.difficulty", "Difficulty", difficulty, Kind::Cycle, 23, 0, soft),
                    ("selectWorld.allowCommands", "Allow Cheats", none(), commands, 24, 0, soft),
                    ("selectWorld.enterSeed", "World Seed", seed.clone(), Kind::Text(1), 1, 1, true),
                    ("selectWorld.mapType", "World Type", world_type, Kind::Cycle, 25, 1, true),
                    ("selectWorld.customizeType", "Customize", none(), Kind::Button, 26, 1, flat),
                    ("selectWorld.mapFeatures", "Structures", none(), structures, 27, 1, true),
                    ("selectWorld.bonusItems", "Bonus Chest", none(), bonus, 28, 1, soft),
                    ("selectWorld.gameRules", "Game Rules", none(), Kind::Button, 29, 2, true),
                    ("selectWorld.experiments", "Experiments", none(), Kind::Button, 30, 2, true),
                    ("selectWorld.dataPacks", "Data Packs", none(), Kind::Button, 31, 2, true),
                ]);
                let create = t("selectWorld.create", "Create New World");
                (create.clone(), (create, 21, true), rows)
            }
            Some(Mode::Edit { name, .. }) => {
                #[rustfmt::skip]
                let rows = self.form_rows(vec![
                    ("selectWorld.enterName", "World Name", name.clone(), Kind::Text(0), 0, 0, true),
                    ("selectWorld.edit.resetIcon", "Reset Icon", none(), Kind::Button, 30, 0, true),
                    ("selectWorld.edit.backup", "Make Backup", none(), Kind::Button, 32, 0, true),
                    ("selectWorld.edit.optimize", "Optimize World", none(), Kind::Button, 34, 0, true),
                ]);
                let save = t("selectWorld.edit.save", "Save");
                let title = t("selectWorld.edit.title", "Edit World");
                (title, (save, 35, !name.trim().is_empty()), rows)
            }
            _ => (none(), (none(), 0, false), Vec::new()),
        }
    }

    /// The text field being typed in, if any (Create's name or seed, Edit's name).
    fn form_typing(&self) -> Option<u8> {
        match self.title.as_ref().map(|t| &t.mode) {
            Some(Mode::Create { focus, .. }) if *focus <= 1 => Some(*focus),
            Some(Mode::Edit { focused: true, .. }) => Some(0),
            _ => None,
        }
    }

    /// Where each row starts, scrolled, and how far the form scrolls.
    fn form_layout(rows: &[Row], height: f32, scroll: f32) -> (Vec<f32>, f32) {
        let mut y = HEADER + 4.0 - scroll;
        let tops = rows
            .iter()
            .map(|r| {
                let top = y;
                y += r.height();
                top
            })
            .collect();
        let total: f32 = rows.iter().map(Row::height).sum();
        (tops, (total + 8.0 - (height - HEADER - 4.0)).max(0.0))
    }

    /// A row's control: the box, choice, switch or button under its label.
    fn form_control(row: &Row, top: f32, width: f32) -> [f32; 4] {
        let w = width - 8.0 - 22.0;
        match row.kind {
            Kind::Button => [8.0, top, w, 18.0],
            Kind::Switch(_) => [8.0, top + 11.0, 26.0, 14.0],
            _ => [8.0, top + 11.0, w, 16.0],
        }
    }

    /// A row tapped or chosen: a text box takes the typing (the keyboard comes up), the rest
    /// press their vanilla button on its tab.
    fn form_activate(&mut self, events: &dyn EventLoopCtl, row: &Row) {
        if !row.enabled {
            return;
        }
        if let Kind::Text(field) = row.kind {
            if let Some(title) = self.title.as_mut() {
                match &mut title.mode {
                    Mode::Create { focus, .. } => *focus = field,
                    Mode::Edit { focused, .. } => *focused = true,
                    _ => {}
                }
            }
            self.android_show_keyboard();
            return;
        }
        // A choice opens its values' dropdown, on the one shown.
        if row.kind == Kind::Cycle
            && let Some((_, at)) = self.form_values(row.id)
        {
            FORM.with_borrow_mut(|f| f.open = Some((row.id, at)));
            return;
        }
        self.form_press(events, row);
    }

    /// `row`'s vanilla button pressed, on its tab, no box taking the typing.
    fn form_press(&mut self, events: &dyn EventLoopCtl, row: &Row) {
        if !row.enabled {
            return;
        }
        if let Some(title) = self.title.as_mut() {
            match &mut title.mode {
                Mode::Create { focus, options, .. } => {
                    *focus = 9;
                    options.tab = row.tab;
                }
                Mode::Edit { focused, .. } => *focused = false,
                _ => {}
            }
        }
        self.press_title_button(events, row.id);
    }

    /// The open dropdown over the form: its values on vanilla's panel, the one shown in
    /// green, the controller's outlined.
    fn draw_form_dropdown(&self, batch: &mut HudBatch, rows: &[Row], tops: &[f32]) {
        let Some((id, pick)) = FORM.with_borrow(|f| f.open) else {
            return;
        };
        let Some((row, &top)) = rows.iter().zip(tops).find(|(r, _)| r.id == id) else {
            return;
        };
        let Some((values, shown)) = self.form_values(id) else {
            return;
        };
        let control = Self::form_control(row, top, batch.width);
        let items = dropdown(control, values.len(), batch.height);
        let (Some(first), Some(last)) = (items.first(), items.last()) else {
            return;
        };
        let atlas = &self.gui.atlas;
        let frame = [first[0], first[1], first[2], last[1] + ITEM - first[1]];
        let [fx, fy, fw, fh] = frame;
        batch.fill(atlas, fx - 1.0, fy - 1.0, fw + 2.0, fh + 2.0, 0xFF20_2020);
        panel(batch, atlas, frame);
        for (i, (rect, value)) in items.iter().zip(&values).enumerate() {
            let [x, y, _, _] = *rect;
            const SHOWN: u32 = 0xFF20_8030;
            let color = if i == shown { SHOWN } else { super::menu::TEXT };
            batch.text(atlas, value, x + 4.0, y + 3.0, color, false);
            if i == pick {
                let inner = [x + 1.0, y + 1.0, rect[2] - 2.0, ITEM - 2.0];
                outline(batch, atlas, inner, u32::MAX);
            }
        }
    }

    pub(super) fn form_frame(
        &mut self,
        events: &dyn EventLoopCtl,
        size: [u32; 2],
        touches: &[Touch],
    ) {
        let mut batch = lower_batch(size);
        let (w, h, scale) = (batch.width, batch.height, batch.scale);
        let (_, header, rows) = self.form_now();
        let (_, max) = Self::form_layout(&rows, h, 0.0);
        FORM.with_borrow_mut(|f| {
            f.height = h;
            f.scroll = f.scroll.clamp(0.0, max);
        });
        let play = [(w / 2.0 - 60.0).floor(), 15.0, 120.0, 16.0];
        for touch in touches {
            match *touch {
                Touch::Down([_, y]) => FORM.with_borrow_mut(|f| {
                    f.finger = Some((y / scale, f.scroll));
                    f.swiped = false;
                }),
                Touch::Move([_, y]) => FORM.with_borrow_mut(|f| {
                    if let Some((from, scroll)) = f.finger {
                        let dy = y / scale - from;
                        f.swiped |= dy.abs() > 4.0;
                        if f.swiped {
                            f.scroll = (scroll - dy).clamp(0.0, max);
                        }
                    }
                }),
                Touch::Up([x, y]) => {
                    let tap = FORM.with_borrow_mut(|f| {
                        f.finger = None;
                        !std::mem::take(&mut f.swiped)
                    });
                    if !tap {
                        continue;
                    }
                    let p = [x / scale, y / scale];
                    // An open dropdown takes the tap: a value chosen, or closed.
                    if let Some((id, _)) = FORM.with_borrow_mut(|f| f.open.take()) {
                        let scroll = FORM.with_borrow(|f| f.scroll);
                        let (tops, _) = Self::form_layout(&rows, h, scroll);
                        let row = rows.iter().zip(&tops).find(|(r, _)| r.id == id);
                        if let Some((row, &top)) = row {
                            let control = Self::form_control(row, top, w);
                            let n = self.form_values(id).map_or(0, |v| v.0.len());
                            if let Some(i) = touch::rect_at(&dropdown(control, n, h), p) {
                                self.form_choose(events, row, i);
                            }
                        }
                        continue;
                    }
                    if inside(play, p) {
                        if header.2 {
                            self.press_title_button(events, header.1);
                        }
                    } else if p[1] > HEADER {
                        let scroll = FORM.with_borrow(|f| f.scroll);
                        let (tops, _) = Self::form_layout(&rows, h, scroll);
                        let hit = rows.iter().zip(&tops).position(|(row, &top)| {
                            let [cx, cy, cw, ch] = Self::form_control(row, top, w);
                            // The label above a control takes the tap too.
                            inside([cx, cy - 11.0, cw.max(w - 30.0), ch + 11.0], p)
                        });
                        if let Some(i) = hit {
                            FORM.with_borrow_mut(|f| f.pad = i);
                            self.form_activate(events, &rows[i]);
                        }
                    }
                    if !self.form_on_second_screen() {
                        return;
                    }
                }
            }
        }
        let (title, header, rows) = self.form_now();
        let (scroll, pad) = FORM.with_borrow(|f| (f.scroll, f.pad));
        self.draw_form_header(&mut batch, &title, &header, play);
        // The rows, clipped under the header.
        let (tops, max) = Self::form_layout(&rows, h, scroll);
        let start = batch.vertices.len();
        for (i, (row, &top)) in rows.iter().zip(&tops).enumerate() {
            self.draw_form_row(&mut batch, row, top, i == pad);
        }
        batch.clip_since(start, 4.0, HEADER + 2.0, w - 4.0, h - 2.0);
        // The scrollbar.
        let [tx, ty, tw, th] = [w - 14.0, HEADER + 4.0, 10.0, h - HEADER - 8.0];
        let atlas = &self.gui.atlas;
        batch.fill(atlas, tx, ty, tw, th, 0xFF37_3737);
        let (thumb, at) = if max > 0.0 {
            let thumb = (th * th / (th + max)).max(10.0);
            (thumb, ty + (th - thumb) * scroll / max)
        } else {
            (th, ty)
        };
        panel(&mut batch, atlas, [tx, at, tw, thumb]);
        self.draw_form_dropdown(&mut batch, &rows, &tops);
        self.present_lower(&mut batch, FORM_BACKGROUND);
    }

    /// The header: the screen's name, and the green button under it (grey when it can't be
    /// pressed).
    fn draw_form_header(
        &self,
        batch: &mut HudBatch,
        title: &str,
        header: &HeaderButton,
        play: [f32; 4],
    ) {
        let atlas = &self.gui.atlas;
        let w = batch.width;
        panel(batch, atlas, [0.0, 0.0, w, HEADER]);
        centered(batch, atlas, title, [0.0, 4.0, w], super::menu::TEXT, false);
        let [px, py, pw, ph] = play;
        let (edge, face) = if header.2 {
            (0xFF20_6020, GREEN)
        } else {
            (0xFF40_4040, 0xFF70_7070)
        };
        batch.fill(atlas, px, py, pw, ph, edge);
        batch.fill(atlas, px + 1.0, py + 1.0, pw - 2.0, ph - 2.0, face);
        centered(batch, atlas, &header.0, [px, py + 4.0, pw], u32::MAX, true);
    }

    /// A row: its label (yellow while chosen), then its box, choice, switch or button.
    fn draw_form_row(&self, batch: &mut HudBatch, row: &Row, top: f32, chosen: bool) {
        let w = batch.width;
        let control = Self::form_control(row, top, w);
        let [cx, cy, cw, ch] = control;
        let atlas = &self.gui.atlas;
        let label_color = match (row.enabled, chosen) {
            (false, _) => 0xFF80_8080,
            (true, true) => 0xFF60_FFFF,
            (true, false) => 0xFFE0_E0E0,
        };
        if row.kind != Kind::Button {
            batch.text(atlas, &row.label, cx, top + 1.0, label_color, true);
        }
        match row.kind {
            Kind::Button => {
                let sprite = if row.enabled {
                    "gui/sprites/widget/button"
                } else {
                    "gui/sprites/widget/button_disabled"
                };
                ui::nine_slice(batch, atlas, sprite, cx, cy, cw, ch, u32::MAX);
                let color = if row.enabled { u32::MAX } else { 0xFFA0_A0A0 };
                centered(batch, atlas, &row.label, [cx, cy + 5.0, cw], color, true);
            }
            Kind::Text(field) => {
                batch.fill(atlas, cx, cy, cw, ch, 0xFF8B_8B8B);
                batch.fill(atlas, cx + 1.0, cy + 1.0, cw - 2.0, ch - 2.0, 0xFF20_2020);
                batch.text(atlas, &row.value, cx + 4.0, cy + 4.0, u32::MAX, false);
                if self.form_typing() == Some(field) {
                    let tw = HudBatch::text_width(atlas, &row.value);
                    batch.fill(atlas, cx + 5.0 + tw, cy + 3.0, 1.0, 10.0, u32::MAX);
                }
            }
            Kind::Cycle => {
                panel(batch, atlas, control);
                let color = if row.enabled {
                    super::menu::TEXT
                } else {
                    0xFF90_9090
                };
                batch.text(atlas, &row.value, cx + 4.0, cy + 4.0, color, false);
                let at = [cx + cw - 12.0, cy + 6.0, 7.0, 4.0];
                arrow(batch, atlas, at, b'v', 0xFF55_5555);
            }
            Kind::Switch(on) => {
                let track = if on && row.enabled {
                    GREEN
                } else {
                    0xFF37_3737
                };
                batch.fill(atlas, cx, cy + 2.0, cw, ch - 4.0, track);
                let knob = if on { cx + cw - 12.0 } else { cx };
                panel(batch, atlas, [knob, cy, 12.0, ch]);
            }
        }
        if chosen {
            outline(batch, atlas, control, u32::MAX);
        }
    }

    /// A controller button on Create New World or Edit World; true when taken: up and down
    /// move over the rows (the form scrolls along), A (or left and right on a choice) uses
    /// the row, X or Start presses Create (or Save).
    pub(super) fn form_pad(
        &mut self,
        events: &dyn EventLoopCtl,
        button: horizon_client::gamepad::Button,
    ) -> bool {
        use horizon_client::gamepad::Button as B;
        let (_, header, rows) = self.form_now();
        if rows.is_empty() {
            return false;
        }
        let at = FORM.with_borrow(|f| f.pad.min(rows.len() - 1));
        // An open dropdown: up and down over its values, A chooses, B closes it.
        if let Some((id, pick)) = FORM.with_borrow(|f| f.open) {
            let n = self.form_values(id).map_or(1, |v| v.0.len()).max(1);
            match button {
                B::DPadUp => FORM.with_borrow_mut(|f| f.open = Some((id, pick.saturating_sub(1)))),
                B::DPadDown => FORM.with_borrow_mut(|f| f.open = Some((id, (pick + 1).min(n - 1)))),
                B::South => {
                    FORM.with_borrow_mut(|f| f.open = None);
                    if let Some(row) = rows.iter().find(|r| r.id == id) {
                        self.form_choose(events, row, pick);
                    }
                }
                B::East => FORM.with_borrow_mut(|f| f.open = None),
                _ => {}
            }
            return true;
        }
        let to = match button {
            B::DPadUp => Some(at.saturating_sub(1)),
            B::DPadDown => Some((at + 1).min(rows.len() - 1)),
            _ => None,
        };
        if let Some(to) = to {
            FORM.with_borrow_mut(|f| {
                f.pad = to;
                let top: f32 = rows[..to].iter().map(Row::height).sum();
                let shown = (f.height - HEADER - 8.0).max(30.0);
                if top < f.scroll {
                    f.scroll = top;
                } else if top + rows[to].height() > f.scroll + shown {
                    f.scroll = top + rows[to].height() - shown;
                }
            });
            return true;
        }
        match button {
            B::South => self.form_activate(events, &rows[at]),
            // A choice steps either way, a switch flips.
            B::DPadLeft | B::DPadRight => match rows[at].kind {
                Kind::Cycle if rows[at].enabled => {
                    if let Some((values, shown)) = self.form_values(rows[at].id) {
                        let n = values.len();
                        let step = if button == B::DPadRight { 1 } else { n - 1 };
                        self.form_choose(events, &rows[at], (shown + step) % n);
                    }
                }
                Kind::Switch(_) => self.form_press(events, &rows[at]),
                _ => {}
            },
            B::West | B::Start if header.2 => self.press_title_button(events, header.1),
            _ => return false,
        }
        true
    }
}
