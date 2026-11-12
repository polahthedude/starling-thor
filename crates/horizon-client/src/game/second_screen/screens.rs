//! The title screen and the pause menu on the dual screen.
//!
//! The title screen: the window keeps the panorama, the logo and the splash; the lower screen
//! has the main buttons (`title_menu`) over the menu background, pressed by id as the keyboard
//! presses them (`focused_button`, `confirm_key_size`), and the player, which a swipe spins.
//! The pause menu, after New 3DS Edition's: its buttons in one column under a title bar on the
//! lower screen (vanilla's actions, `execute_pause_action`), a large pause sign on the window.
use super::menu::{centered, outline, panel};
use super::*;
use crate::second_screen::Touch;
use horizon_client::touch::dual_menu;
use std::time::Instant;

/// Degrees the player turns per GUI pixel of a swipe.
const TURN: f32 = 2.5;

/// The title screen's player.
struct Doll {
    /// Turned this many degrees from facing out.
    yaw: f32,
    /// Degrees a second it keeps spinning after a swipe, easing to rest.
    spin: f32,
    /// The finger spinning it: where it last was across.
    finger: Option<f32>,
    last: Option<Instant>,
}

/// The signed-in account's textures for the title screen's player (`/minecraft/profile`),
/// fetched once, as a world fetches them (`poll_own_profile`).
enum Own {
    Idle,
    Fetching(std::sync::mpsc::Receiver<Option<horizon_client::skins::Textures>>),
    Done(Option<horizon_client::skins::Textures>),
}

/// The title's buttons in the controller's order (ids), and the one it is on.
const TITLE_ORDER: [u8; 8] = [1, 4, 3, 0, 2, 5, 6, 7];

thread_local! {
    static TITLE_FOCUS: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
    static OWN: RefCell<Own> = const { RefCell::new(Own::Idle) };
    static DOLL: RefCell<Doll> = const {
        RefCell::new(Doll {
            yaw: 25.0,
            spin: 0.0,
            finger: None,
            last: None,
        })
    };
}

/// The title's buttons on the lower screen, by id: Singleplayer and Multiplayer across, then
/// Options, Realms and Quit (`Title::buttons`' ids).
const WIDE: [u8; 2] = [1, 4];
const COLUMN: [u8; 3] = [3, 0, 2];
/// Friends, Language and Accessibility, with their sprites.
const ICONS: [(u8, &str); 3] = [
    (5, "gui/sprites/friends/friends"),
    (6, "gui/sprites/icon/language"),
    (7, "gui/sprites/icon/accessibility"),
];

impl Game<'_> {
    /// The title's main screen, its buttons on the second screen.
    pub(super) fn title_on_second_screen(&self) -> bool {
        !matches!(self.screen, Screen::Options | Screen::Mod)
            && self
                .title
                .as_ref()
                .is_some_and(|t| t.buttons_elsewhere && !t.main_buttons().is_empty())
    }

    /// The window's hint bar while a title screen is on the second screen, as New 3DS
    /// Edition's: a dark band along the bottom with the controls.
    pub(in crate::game) fn draw_title_hints(&mut self, batch: &mut HudBatch) {
        if !self.screen_on_second_screen() {
            return;
        }
        let (w, h) = (batch.width, batch.height);
        use horizon_client::title::Mode;
        let mode = self.title.as_ref().map(|t| &t.mode);
        let hint = self.pad_hint(if matches!(mode, Some(Mode::Select { .. })) {
            "{south} Play    {north} Edit    {west} Delete    {east} Back"
        } else if matches!(mode, Some(Mode::Create { .. } | Mode::Edit { .. })) {
            "{south} Select    {west} Create / Save    {east} Back"
        } else {
            "{south} Select    {east} Back"
        });
        let atlas = &self.gui.atlas;
        batch.fill(atlas, 0.0, h - 14.0, w, 14.0, 0xB420_2020);
        centered(batch, atlas, &hint, [0.0, h - 11.0, w], u32::MAX, true);
        self.draw_lower_tooltip(batch, h - 18.0);
    }

    pub(super) fn title_frame(
        &mut self,
        events: &dyn EventLoopCtl,
        size: [u32; 2],
        touches: &[Touch],
    ) {
        let mut batch = HudBatch::new(size);
        batch.scale = touch::dual_panel_scale(size[0]);
        batch.width = (size[0] as f32 / batch.scale).ceil();
        batch.height = (size[1] as f32 / batch.scale).ceil();
        let (w, h, scale) = (batch.width, batch.height, batch.scale);
        let layout = dual_menu::title_menu(w, h);
        let now = Instant::now();
        for touch in touches {
            match *touch {
                Touch::Down([x, y]) => {
                    let p = [x / scale, y / scale];
                    if super::menu::inside(layout.doll, p) {
                        DOLL.with_borrow_mut(|d| (d.finger, d.spin) = (Some(p[0]), 0.0));
                    } else if let Some(id) = Self::title_button_at(&layout, p) {
                        self.press_title(events, id);
                        if !self.title_on_second_screen() {
                            return;
                        }
                    }
                }
                Touch::Move([x, _]) => DOLL.with_borrow_mut(|d| {
                    if let Some(from) = d.finger {
                        // The doll's front follows the finger.
                        let turn = (from - x / scale) * TURN;
                        d.yaw += turn;
                        let dt = d.last.map_or(1.0 / 60.0, |t| (now - t).as_secs_f32());
                        d.spin = turn / dt.max(1.0 / 240.0);
                        d.finger = Some(x / scale);
                    }
                }),
                Touch::Up(_) => DOLL.with_borrow_mut(|d| d.finger = None),
            }
        }
        // Let go, it keeps turning and slows to rest.
        let yaw = DOLL.with_borrow_mut(|d| {
            let dt = d.last.map_or(0.0, |t| (now - t).as_secs_f32()).min(0.1);
            if d.finger.is_none() {
                d.yaw += d.spin * dt;
                d.spin *= (-3.0 * dt).exp();
            }
            d.last = Some(now);
            d.yaw = d.yaw.rem_euclid(360.0);
            d.yaw
        });
        // The title panorama here too, the other way from the window's (another view of the
        // same world, turning with it), a little dimmed under the buttons.
        let view = {
            let mut rotation = self.panorama_rotation;
            rotation.update(0.0, 0.0)
        };
        super::PANORAMA.set(Some([view + 180.0, 10.0]));
        let atlas = &self.gui.atlas;
        batch.fill(atlas, 0.0, 0.0, w, h, 0x5000_0000);
        let buttons = self
            .title
            .as_ref()
            .map(|t| t.main_buttons())
            .unwrap_or_default();
        let find = |id: u8| buttons.iter().find(|b| b.0 == id);
        let labelled = WIDE
            .iter()
            .zip(layout.wide)
            .chain(COLUMN.iter().zip(layout.column));
        let focus = TITLE_FOCUS.get();
        for (&id, rect) in labelled {
            if let Some((_, label, enabled)) = find(id) {
                self.lower_button(&mut batch, rect, label, *enabled, focus == Some(id));
            }
        }
        for ((id, sprite), rect) in ICONS.iter().zip(layout.icons) {
            if let Some((_, _, enabled)) = find(*id) {
                self.lower_button(&mut batch, rect, "", *enabled, focus == Some(*id));
                let atlas = &self.gui.atlas;
                batch.sprite_scaled(
                    atlas,
                    sprite,
                    rect[0] + 3.0,
                    rect[1] + 3.0,
                    15.0,
                    15.0,
                    u32::MAX,
                );
            }
        }
        self.title_skin();
        let [x, y, dw, dh] = layout.doll;
        let size = (dh / 2.3).floor();
        self.draw_player_turned(&mut batch, [x, y, x + dw, y + dh], size, 0.0625, Some(yaw));
        self.present_lower(&mut batch, MENU_BACKGROUND);
    }

    /// The title screen's player wears the player's skin before any world: the launcher's
    /// chosen appearance, else the account's skin (its profile fetched once, the skin
    /// downloaded into an atlas slot as in a world), else an offline name's launcher skin;
    /// the default skin until one has loaded.
    fn title_skin(&mut self) {
        self.flush_skin_slots();
        let account = OWN.with_borrow_mut(|own| match own {
            Own::Idle => {
                *own = match horizon_client::auth::current() {
                    Some(auth) => {
                        let (send, recv) = std::sync::mpsc::channel();
                        std::thread::Builder::new()
                            .name("Own profile".into())
                            .spawn(move || {
                                let _ = send.send(horizon_client::auth::own_textures(&auth));
                            })
                            .ok();
                        Own::Fetching(recv)
                    }
                    None => Own::Done(None),
                };
                None
            }
            Own::Fetching(recv) => {
                if let Ok(textures) = recv.try_recv() {
                    *own = Own::Done(textures);
                }
                None
            }
            Own::Done(textures) => textures.clone(),
        });
        let appearance = horizon_client::skins::appearance::current().and_then(|a| a.skin.clone());
        if appearance.is_none()
            && let Some(textures) = account
        {
            let skin = {
                let mut downloads = self.skin_downloads.borrow_mut();
                horizon_client::skins::lookup(&textures, false, |kind, url| {
                    downloads.state(kind, url)
                })
            };
            if let Some(skin) = skin {
                let (skin, slim) = self.drawn_skin(0, (skin, textures.slim));
                super::super::perspective::set_preview_skin(skin, slim);
                return;
            }
        }
        let local = appearance.or_else(|| horizon_client::skins::offline_skin().cloned());
        if let Some((rgba, slim)) = local {
            const LOCAL: &str = "horizon/offline_skin";
            if !self.skin_slots.contains(LOCAL) || !self.gui.atlas.sprites.contains_key(LOCAL) {
                self.store_skin(LOCAL.into(), rgba);
            }
            let (skin, slim) = self.drawn_skin(0, (LOCAL.into(), slim));
            super::super::perspective::set_preview_skin(skin, slim);
        }
    }

    /// A controller button on the title screen; true when taken: the pad moves over the
    /// buttons, A presses one, L and R spin the player.
    pub(super) fn title_pad(
        &mut self,
        events: &dyn EventLoopCtl,
        button: horizon_client::gamepad::Button,
    ) -> bool {
        use horizon_client::gamepad::Button as B;
        let enabled: Vec<u8> = self
            .title
            .as_ref()
            .map(|t| t.main_buttons())
            .unwrap_or_default()
            .into_iter()
            .filter(|b| b.2)
            .map(|b| b.0)
            .collect();
        let order: Vec<u8> = TITLE_ORDER
            .into_iter()
            .filter(|id| enabled.contains(id))
            .collect();
        if order.is_empty() {
            return false;
        }
        let at = TITLE_FOCUS
            .get()
            .and_then(|id| order.iter().position(|&o| o == id));
        let step = |forward: bool| match at {
            None => 0,
            Some(i) if forward => (i + 1) % order.len(),
            Some(i) => (i + order.len() - 1) % order.len(),
        };
        match button {
            B::DPadDown | B::DPadRight => TITLE_FOCUS.set(Some(order[step(true)])),
            B::DPadUp | B::DPadLeft => TITLE_FOCUS.set(Some(order[step(false)])),
            B::South => match at {
                Some(i) => self.press_title(events, order[i]),
                None => TITLE_FOCUS.set(Some(order[0])),
            },
            B::LeftBumper | B::RightBumper => {
                let turn = if button == B::RightBumper {
                    -540.0
                } else {
                    540.0
                };
                DOLL.with_borrow_mut(|d| d.spin += turn);
            }
            _ => return false,
        }
        true
    }

    /// A controller button on the pause menu; true when taken: up and down move over the
    /// buttons, A presses one (B goes back to the game, as everywhere).
    pub(super) fn pause_pad(&mut self, button: horizon_client::gamepad::Button) -> bool {
        use horizon_client::gamepad::Button as B;
        let ids: Vec<u8> = self.pause_entries().into_iter().map(|e| e.0).collect();
        let at = self
            .pause_focus
            .and_then(|id| ids.iter().position(|&i| i == id));
        match button {
            B::DPadDown => {
                self.pause_focus = Some(ids[at.map_or(0, |i| (i + 1) % ids.len())]);
            }
            B::DPadUp => {
                let i = at.map_or(0, |i| (i + ids.len() - 1) % ids.len());
                self.pause_focus = Some(ids[i]);
            }
            B::South => {
                let id = ids[at.unwrap_or(0)];
                self.pause_focus = Some(id);
                self.audio.play_button_click();
                if let Err(error) = self.execute_pause_action(id) {
                    log::warn!("Second screen: {error}");
                }
            }
            _ => return false,
        }
        true
    }

    fn title_button_at(layout: &dual_menu::TitleMenu, p: [f32; 2]) -> Option<u8> {
        if let Some(i) = touch::rect_at(&layout.wide, p) {
            return Some(WIDE[i]);
        }
        if let Some(i) = touch::rect_at(&layout.column, p) {
            return Some(COLUMN[i]);
        }
        touch::rect_at(&layout.icons, p).map(|i| ICONS[i].0)
    }

    /// A title button pressed as the keyboard presses the focused one.
    fn press_title(&mut self, events: &dyn EventLoopCtl, id: u8) {
        if !self
            .title
            .as_ref()
            .is_some_and(|t| t.main_buttons().iter().any(|b| b.0 == id && b.2))
        {
            return;
        }
        self.press_title_button(events, id);
    }

    /// Any title screen's button pressed as the keyboard presses the focused one (nothing
    /// when the screen has no such button, or it is inactive).
    pub(super) fn press_title_button(&mut self, events: &dyn EventLoopCtl, id: u8) {
        let Some(window) = self
            .renderer
            .as_ref()
            .map(|r| HudBatch::new([r.config.width, r.config.height]))
        else {
            return;
        };
        let Some(title) = self.title.as_mut() else {
            return;
        };
        title.focused_button = Some(id);
        let (action, sound) = title.confirm_key_size((window.width, window.height));
        title.focused_button = None;
        if sound {
            self.audio.play_button_click();
        }
        if let Err(error) = self.title_action(events, action) {
            log::warn!("Second screen: {error}");
        }
    }

    /// Vanilla's button: its sprite (greyed when inactive), the label centered, a yellow
    /// outline when chosen (as the 3DS marks the focused one).
    fn lower_button(
        &self,
        batch: &mut HudBatch,
        rect: [f32; 4],
        label: &str,
        enabled: bool,
        chosen: bool,
    ) {
        let atlas = &self.gui.atlas;
        let [x, y, w, h] = rect;
        // Focus is vanilla's: the button's highlighted (white-edged) sprite.
        let sprite = if !enabled {
            "gui/sprites/widget/button_disabled"
        } else if chosen {
            "gui/sprites/widget/button_highlighted"
        } else {
            "gui/sprites/widget/button"
        };
        ui::nine_slice(batch, atlas, sprite, x, y, w, h, u32::MAX);
        let color = if enabled { u32::MAX } else { 0xFFA0_A0A0 };
        centered(
            batch,
            atlas,
            label,
            [x, y + ((h - 8.0) / 2.0).floor(), w],
            color,
            true,
        );
    }

    /// The pause menu's buttons: vanilla's, in its order, one column.
    fn pause_entries(&self) -> Vec<(u8, String)> {
        let t = |key: &str, plain: &str| self.translate(key, plain);
        let mut v = vec![
            (0, t("menu.returnToGame", "Back to Game")),
            (4, t("gui.advancements", "Advancements")),
            (3, t("gui.stats", "Statistics")),
            (1, t("menu.options", "Options...")),
            (9, t("options.worldOptions.button", "World Options...")),
        ];
        if self.friends_view().shown {
            v.push((10, t("gui.friends.open", "Friends")));
        }
        if !self.server_links().is_empty() {
            v.push((11, t("menu.server_links", "Server Links...")));
        }
        v.push(if self.external_server() {
            (2, t("menu.disconnect", "Disconnect"))
        } else {
            (2, t("menu.returnToMenu", "Save and Quit to Title"))
        });
        v
    }

    pub(super) fn pause_frame(&mut self, size: [u32; 2], taps: Vec<[f32; 2]>) {
        let mut batch = lower_batch(size);
        let (w, h) = (batch.width, batch.height);
        let entries = self.pause_entries();
        // Inside the frame: 6 kept clear of its bottom border.
        let (title, rects) = dual_menu::pause_menu(w, h - 6.0, entries.len());
        for [x, y] in taps {
            let p = [x / batch.scale, y / batch.scale];
            if let Some(i) = touch::rect_at(&rects, p) {
                let id = entries[i].0;
                self.pause_focus = Some(id);
                self.audio.play_button_click();
                self.android_touch_haptic(12);
                if let Err(error) = self.execute_pause_action(id) {
                    log::warn!("Second screen: {error}");
                }
                if self.screen != Screen::Paused {
                    return;
                }
            }
        }
        // As the panel: the world's sky, and the frame holding the title (with the pause
        // sign) and the buttons.
        self.draw_panel_sky(&mut batch, [0.0, 0.0, w, h], [0.0, 0.0, w, h], false);
        super::draw_frame(&mut batch, &self.gui.atlas, [2.0, 2.0, w - 4.0, h - 4.0]);
        let [_, ty, _, _] = title;
        let label = self.translate("menu.paused", "Game Paused");
        let atlas = &self.gui.atlas;
        let ty = ty + 10.0;
        let text = super::menu::TEXT;
        centered(&mut batch, atlas, &label, [0.0, ty, w], text, false);
        for dx in [12.0, 17.0] {
            batch.fill(atlas, dx, ty - 1.0, 3.0, 9.0, text);
        }
        let focus = self.pause_focus;
        for ((id, label), rect) in entries.iter().zip(rects) {
            self.lower_button(&mut batch, rect, label, true, focus == Some(*id));
        }
        self.present_lower(&mut batch, MENU_BACKGROUND);
    }

    /// A world loading (or a server being joined): Minecraft's own loading screen, with the
    /// chunk map, on the lower screen; the window has the bar alone (`remote_overlay`).
    pub(super) fn loading_frame(&mut self, size: [u32; 2]) {
        use horizon_client::level_loading;
        let mut batch = HudBatch::new(size);
        self.draw_level_loading_background(&mut batch, level_loading::Reason::Other);
        {
            let atlas = &self.gui.atlas;
            match self.integrated.as_ref().map(|i| i.load_view()) {
                Some(view) => {
                    let status = |x: i32, z: i32| view.status(x, z);
                    let progress = view.has_progress().then_some(self.loading_progress);
                    level_loading::draw(&mut batch, atlas, Some(&status), progress);
                }
                None => level_loading::draw(&mut batch, atlas, None, None),
            }
        }
        self.present_lower(&mut batch, MENU_BACKGROUND);
    }

    /// The sign being written, as New 3DS Edition's prompt but with the system keyboard: the
    /// sign itself on the window (its editor's own drawing); here its four lines, the one
    /// being written outlined, a tap on one writes it (the keyboard comes up), and Done.
    pub(super) fn sign_frame(&mut self, size: [u32; 2], taps: Vec<[f32; 2]>) {
        let mut batch = lower_batch(size);
        let (w, band) = (batch.width, 24.0);
        let header = [0.0, band + 2.0, w, 16.0];
        let rows: [[f32; 4]; 4] =
            std::array::from_fn(|i| [8.0, band + 22.0 + i as f32 * 22.0, w - 16.0, 18.0]);
        let done = [(w / 2.0 - 50.0).floor(), rows[3][1] + 26.0, 100.0, 20.0];
        for [x, y] in taps {
            let p = [x / batch.scale, y / batch.scale];
            if super::menu::inside(done, p) {
                self.audio.play_button_click();
                self.close_sign_editor();
                return;
            }
            if let Some(line) = touch::rect_at(&rows, p) {
                self.sign_go_line(line);
                self.android_show_keyboard();
            } else if let Some(slot) = lower_hotbar_slot(w, band, p)
                && !self.spectator()
            {
                self.selected = slot;
            }
        }
        self.draw_lower_hotbar(&mut batch, band);
        let title = self.translate("sign.edit", "Edit Sign Message");
        let atlas = &self.gui.atlas;
        panel(&mut batch, atlas, header);
        centered(
            &mut batch,
            atlas,
            &title,
            [0.0, header[1] + 4.0, w],
            super::menu::TEXT,
            false,
        );
        let (lines, current) = self.sign_lines_now();
        for (i, rect) in rows.into_iter().enumerate() {
            let atlas = &self.gui.atlas;
            let [x, y, rw, rh] = rect;
            batch.fill(atlas, x, y, rw, rh, 0xFF37_3737);
            batch.fill(atlas, x + 1.0, y + 1.0, rw - 2.0, rh - 2.0, 0xFF1E_1E1E);
            if let Some(text) = lines.get(i) {
                batch.text(atlas, text, x + 4.0, y + 5.0, u32::MAX, true);
            }
            if i == current {
                outline(&mut batch, atlas, rect, u32::MAX);
            }
        }
        let label = self.translate("gui.done", "Done");
        self.lower_button(&mut batch, done, &label, true, false);
        self.present_lower(&mut batch, BACKGROUND);
    }

    /// The sign's four lines as written, and the one being written.
    fn sign_lines_now(&self) -> (Vec<String>, usize) {
        use horizon_client::containers::BlockEntity;
        let Some(edit) = self.sign_edit.as_ref() else {
            return (Vec::new(), 0);
        };
        let Some(BlockEntity::Sign(sign)) = self.world.block_entities.get(&edit.pos) else {
            return (Vec::new(), edit.line);
        };
        let side = if edit.front { &sign.front } else { &sign.back };
        (side.lines.iter().cloned().collect(), edit.line)
    }

    /// Move the sign's editing to `line`, as Down does.
    fn sign_go_line(&mut self, line: usize) {
        let current = self.sign_edit.as_ref().map_or(0, |e| e.line);
        for _ in 0..(line + 4 - current) % 4 {
            self.sign_input(Some(winit::keyboard::KeyCode::ArrowDown), None);
        }
    }

    /// A controller button while a sign is written; true when taken: up and down choose the
    /// line, A brings the keyboard up, B or Start is Done.
    pub(super) fn sign_pad(&mut self, button: horizon_client::gamepad::Button) -> bool {
        use horizon_client::gamepad::Button as B;
        use winit::keyboard::KeyCode;
        match button {
            B::DPadUp => self.sign_input(Some(KeyCode::ArrowUp), None),
            B::DPadDown => self.sign_input(Some(KeyCode::ArrowDown), None),
            B::South => self.android_show_keyboard(),
            B::East | B::Start => self.close_sign_editor(),
            _ => return false,
        }
        true
    }

    /// The window over the death screen (moved to the lower screen with its buttons): the red,
    /// "You Died!", how, and the score, as vanilla's.
    pub(super) fn draw_death_upper(&mut self, batch: &mut HudBatch) {
        use horizon_client::text::Text;
        let atlas = &self.gui.atlas;
        let (w, h) = (batch.width, batch.height);
        batch.fill_gradient(
            atlas,
            (0.0, 0.0, w, h),
            rgba(80, 0, 0, 96),
            rgba(128, 48, 48, 160),
        );
        let key = if self.level.hardcore {
            "deathScreen.title.hardcore"
        } else {
            "deathScreen.title"
        };
        let title = Text::translate(key, &[]);
        let tw = atlas.font.width_text(&title);
        let y = (h * 0.3).floor();
        ui::text_scaled(
            batch,
            atlas,
            &title,
            (w / 2.0 - tw).floor(),
            y,
            2.0,
            u32::MAX,
        );
        if let Some(cause) = self.death_cause() {
            let cw = atlas.font.width_text(cause);
            batch.text_styled(
                atlas,
                cause,
                ((w - cw) / 2.0).floor(),
                y + 25.0,
                u32::MAX,
                true,
            );
        }
        let score = Text::translate(
            "deathScreen.score.value",
            &[Text::colored(
                self.experience.total.to_string(),
                horizon_client::text::YELLOW,
            )],
        );
        let sw = atlas.font.width_text(&score);
        batch.text_styled(
            atlas,
            &score,
            ((w - sw) / 2.0).floor(),
            y + 40.0,
            u32::MAX,
            true,
        );
    }

    /// The window over a screen moved to the lower screen without a layout of its own: its
    /// name and the controls along the bottom, over the darkened world, as the inventory's.
    pub(super) fn draw_screen_header(&mut self, batch: &mut HudBatch) {
        let (key, plain) = match self.screen {
            Screen::Options => ("options.title", "Options"),
            Screen::Stats => ("gui.stats", "Statistics"),
            Screen::Advancements => ("gui.advancements", "Advancements"),
            Screen::WorldOptions => ("options.worldOptions.button", "World Options"),
            Screen::Friends => ("gui.friends.open", "Friends"),
            Screen::ServerLinks => ("menu.server_links", "Server Links"),
            Screen::CommandBlock => ("advMode.setCommand", "Command Block"),
            Screen::DebugOptions => ("debug.options.title", "Debug Options"),
            _ => ("menu.game", "Game Menu"),
        };
        let title = self.translate(key, plain);
        let hint = self.pad_hint(match self.screen {
            Screen::Stats => "{up}/{down} Scroll    {lb}/{rb} Tab    {east} Back",
            Screen::Advancements => "{up}{down}{left}{right} Move    {lb}/{rb} Tab    {east} Back",
            Screen::Options => "{south} Select    {left}/{right} Change    {east} Back",
            _ => "{south} Select    {east} Back",
        });
        // The caption as the inventory's: the name along the bottom, the controls under it,
        // a tooltip of the screen above them.
        self.draw_upper_caption(batch, None, Some(&title), &hint);
        let h = batch.height;
        self.draw_lower_tooltip(batch, h - 46.0);
    }

    /// The window under the pause menu: a large pause sign and the menu's name, over the world.
    pub(super) fn draw_pause_upper(&mut self, batch: &mut HudBatch) {
        let (w, h) = (batch.width, batch.height);
        // The sign (28 tall) and the name under it (34 below its middle, 8 tall), centered.
        let (cx, cy) = ((w / 2.0).floor(), (h / 2.0 - 14.0).floor());
        let atlas = &self.gui.atlas;
        for dx in [-10.0, 3.0] {
            batch.fill(atlas, cx + dx, cy - 14.0, 7.0, 28.0, u32::MAX);
        }
        let title = self.translate("menu.game", "Game Menu");
        let atlas = &self.gui.atlas;
        centered(batch, atlas, &title, [0.0, cy + 34.0, w], u32::MAX, true);
    }
}
