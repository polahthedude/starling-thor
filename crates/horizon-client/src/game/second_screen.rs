//! Dual-screen handhelds (AYN Thor...): what the game draws on the second display
//! (`android/second_screen.rs` brings its surface), laid out after New 3DS Edition's touch screen
//! (`touch::dual_panel`): the hotbar along the top, the coordinates under it, the map at the left
//! and three buttons (inventory, crafting, pause) at the right, in vanilla's sprites. The window
//! keeps the world and the bars, which `hud::dual_screen_bars` stacks from its top edge.
use super::*;
use crate::second_screen::{Change, NativeWindow, closed, take_change};
use horizon_client::{
    gfx::second_screen::SecondScreen,
    hud::{self, MINIMAP_SPRITE, rgba},
    touch, ui,
};
use std::cell::{Cell, RefCell};

mod container;
mod creative;
mod forms;
mod menu;
mod screens;
mod worlds;

/// The hotbar's top edge: 1 pixel down, in the band's 24.
const HOTBAR_TOP: f32 = 1.0;

/// Vanilla's container grey (`#C6C6C6`), as the touch screen's background.
const BACKGROUND: wgpu::Color = wgpu::Color {
    r: 198.0 / 255.0,
    g: 198.0 / 255.0,
    b: 198.0 / 255.0,
    a: 1.0,
};

/// The second display while Android gives it a surface. It outlives a `Game` (back to the
/// launcher and in again): what draws on it is made again for each renderer's device.
struct Lower {
    /// Dropped before the window it borrows (field order).
    drawn: Option<SecondScreen>,
    window: NativeWindow,
    size: [u32; 2],
    /// Making `drawn` failed on this window: not tried again every frame.
    failed: bool,
}

/// Around a screen: the near-black of vanilla's dimmed background (`#101010`).
const MENU_BACKGROUND: wgpu::Color = wgpu::Color {
    r: 16.0 / 255.0,
    g: 16.0 / 255.0,
    b: 16.0 / 255.0,
    a: 1.0,
};

thread_local! {
    static SCREEN: RefCell<Option<Lower>> = const { RefCell::new(None) };
    /// The window's HUD drew this frame and left the hotbar to this screen.
    static HOTBAR: Cell<bool> = const { Cell::new(false) };
    /// The hotbar row's scale and shift (`hud::fit_row`) as last drawn, eased toward the fit
    /// that frame's row needs, and when.
    static FIT: Cell<(f32, f32, Option<std::time::Instant>)> = const { Cell::new((1.0, 0.0, None)) };
    /// A moved screen is being drawn here, with the controller's pointer or focus.
    static DRAWING_LOWER: Cell<bool> = const { Cell::new(false) };
    /// The moved screen's tooltip this frame: shown on the window, off the buttons.
    static LOWER_TOOLTIP: RefCell<Vec<horizon_client::text::Text>> = const { RefCell::new(Vec::new()) };
}

/// The controller's pointer or focus is being drawn on the second screen (`draw_pad_cursor`).
pub(super) fn drawing_lower() -> bool {
    DRAWING_LOWER.get()
}

/// A batch for the lower screen at its own GUI scale: the hotbar about as wide as the screen,
/// as on the 3DS.
fn lower_batch(size: [u32; 2]) -> HudBatch {
    lower_batch_at(size, touch::dual_panel_scale(size[0]))
}

/// A batch for the lower screen at GUI scale `scale`.
fn lower_batch_at(size: [u32; 2], scale: f32) -> HudBatch {
    let mut batch = HudBatch::new(size);
    batch.scale = scale;
    batch.width = (size[0] as f32 / batch.scale).ceil();
    batch.height = (size[1] as f32 / batch.scale).ceil();
    batch
}

/// The hotbar slot at `point` on the lower screen, `width` wide with the hotbar in its top
/// `band`: back through the row's fit, as drawn.
fn lower_hotbar_slot(width: f32, band: f32, point: [f32; 2]) -> Option<usize> {
    let (scale, dx, _) = FIT.get();
    let unfit = [
        (point[0] - dx - width / 2.0) / scale + width / 2.0,
        (point[1] - HOTBAR_TOP) / scale + HOTBAR_TOP,
    ];
    touch::hotbar_slot(width, band - 1.0, unfit)
}

/// Hotbar slot `slot`'s box on the lower screen, `width` wide, as drawn (through the row's
/// fit): the hotbar's top at `HOTBAR_TOP`, its slots 20 apart from 1 inside its left end.
fn lower_hotbar_rect(width: f32, slot: usize) -> [f32; 4] {
    let (scale, dx, _) = FIT.get();
    let x = ((width as i32) / 2 - 91) as f32 + 1.0 + slot as f32 * 20.0;
    let fit_x = |x: f32| (x - width / 2.0) * scale + width / 2.0 + dx;
    let fit_y = |y: f32| (y - HOTBAR_TOP) * scale + HOTBAR_TOP;
    [
        fit_x(x),
        fit_y(HOTBAR_TOP + 1.0),
        20.0 * scale,
        20.0 * scale,
    ]
}

/// The touch id a finger on the second screen has among the window's: Android's are small.
pub(super) const LOWER_FINGER: u64 = 1 << 40;

/// The game is ending: what it drew the second screen with goes now, with its renderer still
/// there (a surface outliving its device and instance is what wgpu can't take); the display's
/// window stays for the next game.
pub(crate) fn release() {
    SCREEN.with_borrow_mut(|screen| {
        if let Some(lower) = screen {
            lower.drawn = None;
            lower.failed = false;
        }
    });
    // Nothing draws on the window now: Android may destroy it without waiting.
    closed();
}

/// A second screen is drawn on: the window's HUD leaves the hotbar to it.
pub(super) fn active() -> bool {
    SCREEN.with_borrow(|s| s.as_ref().is_some_and(|l| l.drawn.is_some()))
}

impl Game<'_> {
    /// `second_screen_pad` takes the controller's directions, and the window's menu pointer
    /// stays away: the second screen's own layouts, and Options, the statistics and the
    /// advancements moved there (driven as `options_pad`, `stats_pad`, `advancements_pad`).
    pub(super) fn second_screen_owns_pad(&self) -> bool {
        self.second_screen_native()
            || (self.screen_on_second_screen()
                && matches!(
                    self.screen,
                    Screen::Options | Screen::Stats | Screen::Advancements
                ))
    }

    /// The second screen's own layouts are up (not a screen moved there whole).
    fn second_screen_native(&self) -> bool {
        if !active() {
            return false;
        }
        if self.sign_edit.is_some()
            || self.title_on_second_screen()
            || self.worlds_on_second_screen()
            || self.form_on_second_screen()
        {
            return true;
        }
        self.screen_on_second_screen()
            && match self.screen {
                Screen::Paused | Screen::Creative => true,
                Screen::Inventory => {
                    self.crafting_screen_open()
                        || self
                            .renderer
                            .as_ref()
                            .map(|r| HudBatch::new([r.config.width, r.config.height]))
                            .is_some_and(|w| self.container_region(&w).is_some())
                }
                _ => false,
            }
    }

    /// A controller button while a screen of the second screen is up; true when it took it
    /// (the rest, B and Start among them, go on to the window's menu handling).
    pub(super) fn second_screen_pad(
        &mut self,
        events: &dyn EventLoopCtl,
        button: horizon_client::gamepad::Button,
    ) -> bool {
        if !active() {
            return false;
        }
        if self.sign_edit.is_some() {
            return self.sign_pad(button);
        }
        if self.title_on_second_screen() {
            return self.title_pad(events, button);
        }
        if self.worlds_on_second_screen() {
            return self.worlds_pad(events, button);
        }
        if self.form_on_second_screen() {
            return self.form_pad(events, button);
        }
        if !self.screen_on_second_screen() {
            return false;
        }
        let owned = self.second_screen_owns_pad();
        let taken = match self.screen {
            Screen::Paused => self.pause_pad(button),
            Screen::Creative => self.creative_pad(button),
            Screen::Inventory if self.crafting_screen_open() => self.legacy_pad(button),
            Screen::Inventory => self.container_pad(button),
            Screen::Options => self.options_pad(button),
            Screen::Stats => self.stats_pad(button),
            Screen::Advancements => self.advancements_pad(button),
            _ => false,
        };
        // The rest stay off the window's hidden screen (its pointer would click there), but
        // B and Start, which go back.
        use horizon_client::gamepad::Button as B;
        taken || (owned && !matches!(button, B::East | B::Start))
    }
}

/// Called by the window's HUD where it would have drawn the hotbar.
pub(super) fn show_hotbar() {
    HOTBAR.set(true);
}

impl Game<'_> {
    pub(super) fn second_screen_frame(&mut self, events: &dyn EventLoopCtl, id: WindowId) {
        SCREEN.with_borrow_mut(|screen| match take_change() {
            Some(Change::Open(window, size)) => {
                // Android gives one surface per window: the old one goes first.
                *screen = None;
                *screen = Some(Lower {
                    drawn: None,
                    window,
                    size,
                    failed: false,
                });
                // Showing it brought the system bars back on the window.
                crate::native_ime::reapply_screen();
            }
            Some(Change::Close) => {
                *screen = None;
                closed();
            }
            None => {}
        });
        if let Some(renderer) = self.renderer.as_ref() {
            SCREEN.with_borrow_mut(|screen| {
                let Some(lower) = screen else {
                    return;
                };
                // Made by an earlier renderer (a game before this one, or the graphics
                // settings rebuilt it): its device is gone.
                if lower.drawn.as_ref().is_some_and(|d| !renderer.owns(d)) {
                    lower.drawn = None;
                    lower.failed = false;
                }
                if lower.drawn.is_some() || lower.failed {
                    return;
                }
                // A failure here (a panic in the driver's or wgpu's surface code too) leaves
                // the second screen dark until it is shown again, not the game ended.
                let (window, size) = (&lower.window, lower.size);
                let opened = horizon_client::crash::contain(|| {
                    window
                        .target()
                        .and_then(|target| {
                            // SAFETY: `Lower` drops the screen before the window.
                            unsafe { renderer.wgpu_instance().create_surface_unsafe(target) }
                                .map_err(|e| e.to_string())
                        })
                        .map_err(Into::into)
                        .and_then(|surface| renderer.second_screen(surface, size))
                        .map_err(|e| e.to_string())
                })
                .and_then(|opened| opened);
                match opened {
                    Ok(drawn) => {
                        lower.drawn = Some(drawn);
                        crate::second_screen::drawing();
                    }
                    Err(error) => {
                        log::warn!("Second screen: {error}");
                        lower.failed = true;
                    }
                }
            });
        }
        let hotbar = HOTBAR.replace(false);
        let touches = crate::second_screen::take_touches();
        let taps: Vec<[f32; 2]> = touches
            .iter()
            .filter_map(|t| match t {
                crate::second_screen::Touch::Down(p) => Some(*p),
                _ => None,
            })
            .collect();
        // The title screen's main buttons move here while this screen is drawn on.
        let lower = SCREEN.with_borrow(|s| s.as_ref().is_some_and(|l| l.drawn.is_some()));
        if let Some(title) = self.title.as_mut() {
            title.buttons_elsewhere = lower;
        }
        let size = SCREEN.with_borrow(|s| s.as_ref()?.drawn.as_ref().map(|d| d.size()));
        let Some(size) = size else {
            return;
        };
        if !(self.screen_on_second_screen() && self.crafting_screen_open()) {
            menu::closed();
        }
        if self.title_on_second_screen() {
            return self.title_frame(events, size, &touches);
        }
        if self.worlds_on_second_screen() {
            return self.worlds_frame(events, size, &touches);
        }
        if self.form_on_second_screen() {
            return self.form_frame(events, size, &touches);
        }
        // A world loading or being joined, and a sign being written, have their own here.
        if self.remote_loading() {
            return self.loading_frame(size);
        }
        if self.sign_edit.is_some() {
            return self.sign_frame(size, taps);
        }
        if self.screen_on_second_screen() {
            return self.second_screen_menu(events, id, size, taps, &touches);
        }
        let mut batch = lower_batch(size);
        if hotbar {
            let panel = touch::dual_panel(batch.width, batch.height);
            for [x, y] in taps {
                self.second_screen_tap(&panel, [x / batch.scale, y / batch.scale]);
            }
            self.draw_panel(&mut batch, &panel);
        }
        self.present_lower(&batch, BACKGROUND);
    }

    /// While alive, a screen moved whole to the second screen (not one of its own layouts)
    /// lays out at that screen's size: at vanilla's scale for it, or larger for the
    /// advancements' fixed window so it fills more of it.
    pub(super) fn lower_layout(&self) -> hud::LayoutAs {
        let window = self
            .renderer
            .as_ref()
            .map_or([1, 1], |r| [r.config.width, r.config.height]);
        // Trading and the beacon are the inventory screens moved whole.
        let moved = self.screen_on_second_screen() && !self.second_screen_native();
        let size = SCREEN
            .with_borrow(|s| s.as_ref()?.drawn.as_ref().map(|d| d.size()))
            .filter(|_| moved);
        let min = match self.screen {
            Screen::Advancements => [256, 170],
            // Trading's 276 by 166 and the beacon's 230 by 219 windows, larger.
            Screen::Inventory => [280, 222],
            _ => [320, 240],
        };
        hud::LayoutAs::new(window, size.map(|s| (s, min)))
    }

    /// Options' GUI size while it is open on the second screen: that screen's own, at vanilla's
    /// scale for it (`Window.calculateScale`).
    pub(super) fn lower_options_size(&self) -> Option<(f32, f32)> {
        if self.screen != Screen::Options || !self.screen_on_second_screen() {
            return None;
        }
        let size = SCREEN.with_borrow(|s| s.as_ref()?.drawn.as_ref().map(|d| d.size()))?;
        let batch = HudBatch::new(size);
        Some((batch.width, batch.height))
    }

    /// A controls hint in the connected controller's glyphs: each `{name}` (`Button::name`:
    /// `{south}`, `{lb}`...) becomes that button's label.
    pub(super) fn pad_hint(&self, text: &str) -> String {
        use horizon_client::gamepad::Button;
        let style = self.ui.options.pad_style;
        let mut hint = text.to_owned();
        for button in Button::ALL {
            hint = hint.replace(&format!("{{{}}}", button.name()), button.label(style));
        }
        hint
    }

    /// The tooltip of the screen moved to the second screen, on the window above `bottom`.
    pub(super) fn draw_lower_tooltip(&self, batch: &mut HudBatch, bottom: f32) {
        if !self.screen_on_second_screen() {
            return;
        }
        LOWER_TOOLTIP.with_borrow(|lines| {
            if !lines.is_empty() {
                let x = (batch.width / 2.0).floor();
                ui::tooltip_above(batch, &self.gui.atlas, lines, x, bottom);
            }
        });
    }

    /// Draw `batch` on the second screen over `clear`.
    fn present_lower(&mut self, batch: &HudBatch, clear: wgpu::Color) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        SCREEN.with_borrow_mut(|screen| {
            let Some(lower) = screen.as_mut() else {
                return;
            };
            let Some(drawn) = lower.drawn.as_mut() else {
                return;
            };
            let drew =
                horizon_client::crash::contain(|| renderer.draw_second_screen(drawn, batch, clear));
            if let Err(error) = drew {
                log::error!("Second screen: {error}; it stays dark until shown again");
                lower.drawn = None;
                lower.failed = true;
            }
        });
    }

    /// A left click at `gui` (the window's GUI pixels), as a touch on the window gives one: the
    /// cursor moved there, pressed and let go, through the window's own input handling.
    fn click_window_at(&mut self, events: &dyn EventLoopCtl, id: WindowId, gui: [f32; 2]) {
        self.window_mouse(events, id, gui, Some(ElementState::Pressed));
        self.window_mouse(events, id, gui, Some(ElementState::Released));
    }

    /// The window's mouse at `gui` (its GUI pixels), the left button pressed or let go there
    /// when `state` says so: a finger on the second screen moving the window's pointer, so
    /// sliders, scrollbars and drags work as on the window.
    fn window_mouse(
        &mut self,
        events: &dyn EventLoopCtl,
        id: WindowId,
        gui: [f32; 2],
        state: Option<ElementState>,
    ) {
        let Some(scale) = self
            .renderer
            .as_ref()
            .map(|r| HudBatch::new([r.config.width, r.config.height]).scale)
        else {
            return;
        };
        let device_id = winit::event::DeviceId::dummy();
        let position =
            winit::dpi::PhysicalPosition::new(f64::from(gui[0] * scale), f64::from(gui[1] * scale));
        self.handle_window_event(
            events,
            id,
            WindowEvent::CursorMoved {
                device_id,
                position,
            },
        );
        if let Some(state) = state {
            let button = MouseButton::Left;
            let input = WindowEvent::MouseInput {
                device_id,
                state,
                button,
            };
            self.handle_window_event(events, id, input);
        }
    }

    /// The open screen as the window would draw it (the screens without a layout of their own
    /// here).
    fn draw_moved_screen(&mut self, batch: &mut HudBatch) {
        // The title's screens (worlds, multiplayer, confirmations...).
        if !matches!(self.screen, Screen::Options | Screen::Mod)
            && let Some(title) = &self.title
        {
            title.draw_with_items(
                &self.gui.atlas,
                Some(&self.pack.item_renderer),
                batch,
                self.cursor,
                true,
            );
            return;
        }
        match self.screen {
            Screen::Options => {
                self.flush_skin_slots();
                self.update_remote_skins();
                self.draw_options(batch);
            }
            Screen::Dead => self.draw_death_screen(batch),
            Screen::Stats => self.draw_stats(batch),
            Screen::Advancements => self.draw_advancements(batch),
            Screen::WorldOptions => self.draw_world_options(batch),
            Screen::Friends => self.draw_friends_screen(batch),
            Screen::ServerLinks => self.draw_server_links(batch),
            Screen::ConfirmLink => self.draw_confirm_link(batch),
            Screen::DialogWarning => self.draw_dialog_warning(batch),
            Screen::CommandBlock => self.draw_command_block(batch),
            Screen::BlockEditor => self.draw_block_editor(batch),
            Screen::DebugOptions => self.draw_debug_options(batch),
            Screen::Mod => self.draw_mod_screen(batch),
            Screen::Creative => self.draw_creative(batch),
            Screen::Inventory => {
                self.draw_inventory(batch);
                self.draw_mod_browser(batch);
            }
            _ => {}
        }
    }

    /// The open screen on the second screen, as New 3DS Edition opens its menus on its touch
    /// screen: the pause menu, inventory, crafting, creative and containers in layouts of their
    /// own; any other screen (options, statistics, trading...) whole, laid out, hit-tested and
    /// drawn at the second screen's size (`lower_layout`), a finger there its touch.
    fn second_screen_menu(
        &mut self,
        events: &dyn EventLoopCtl,
        id: WindowId,
        size: [u32; 2],
        taps: Vec<[f32; 2]>,
        touches: &[crate::second_screen::Touch],
    ) {
        // The inventory, crafting, creative and pause screens have their own here.
        if self.screen == Screen::Paused {
            return self.pause_frame(size, taps);
        }
        if self.crafting_screen_open() {
            return self.legacy_frame(size, touches);
        }
        if self.screen == Screen::Creative {
            return self.creative_frame(size, touches);
        }
        if self.screen == Screen::Inventory && self.container_frame(events, id, size, touches) {
            return;
        }
        // The screen lays itself out, is hit-tested and drawn at this screen's size
        // (`lower_layout`): the window's batches are this screen's while it is handled.
        let _layout = self.lower_layout();
        let Some(window) = self
            .renderer
            .as_ref()
            .map(|r| HudBatch::new([r.config.width, r.config.height]))
        else {
            return;
        };
        let mut batch = HudBatch::new(size);
        (batch.scale, batch.width, batch.height) = (window.scale, window.width, window.height);
        // A finger is a finger on the window's menus (`android_menu_touch`): a tap clicks on
        // release, a drag scrolls the list, a slider follows it, a text field brings the
        // keyboard.
        use crate::second_screen::Touch;
        use winit::event::TouchPhase;
        for touch in touches {
            let ([x, y], phase) = match *touch {
                Touch::Down(p) => (p, TouchPhase::Started),
                Touch::Move(p) => (p, TouchPhase::Moved),
                Touch::Up(p) => (p, TouchPhase::Ended),
            };
            let gui = [x / batch.scale, y / batch.scale];
            let touch = winit::event::Touch {
                device_id: winit::event::DeviceId::dummy(),
                phase,
                location: winit::dpi::PhysicalPosition::new(
                    f64::from(gui[0] * window.scale),
                    f64::from(gui[1] * window.scale),
                ),
                force: None,
                id: LOWER_FINGER,
            };
            self.android_menu_touch(&touch, events, id);
            // The tap may have closed the screen (or the game).
            if !self.screen_on_second_screen() {
                return;
            }
        }
        drop(taps);
        ui::tooltips_elsewhere(true);
        self.draw_moved_screen(&mut batch);
        LOWER_TOOLTIP.set(ui::tooltips_elsewhere(false));
        DRAWING_LOWER.set(true);
        self.draw_pad_cursor(&mut batch);
        DRAWING_LOWER.set(false);
        // The death screen's red reaches the edges.
        let clear = if self.screen == Screen::Dead {
            wgpu::Color {
                r: 0.42,
                g: 0.10,
                b: 0.10,
                a: 1.0,
            }
        } else {
            MENU_BACKGROUND
        };
        self.present_lower(&batch, clear);
    }

    fn second_screen_tap(&mut self, panel: &touch::DualPanel, point: [f32; 2]) {
        // A hotbar slot is selected, as on the window's touch controls; the next tick tells
        // the server (SetCarriedItem).
        if let Some(slot) = lower_hotbar_slot(panel.coords[2], panel.band, point) {
            if !self.spectator() {
                self.selected = slot;
            }
            return;
        }
        let inventory = "action:key.inventory";
        match touch::rect_at(&panel.buttons, point) {
            // The inventory (the 3DS's X button), or crafting (Y): one screen, its tabs.
            Some(button @ (0 | 1)) => {
                menu::open_on(if button == 0 { None } else { Some(0) });
                if self.screen != Screen::Inventory {
                    self.android_dispatch_custom_token(inventory, true);
                    self.android_dispatch_custom_token(inventory, false);
                }
            }
            // Pause, as the touch controls' Escape: the menu, or back out of a screen.
            Some(_) => {
                if let Err(error) = self.pad_back() {
                    log::warn!("Second screen: {error}");
                }
            }
            None => {}
        }
    }

    fn draw_panel(&mut self, batch: &mut HudBatch, panel: &touch::DualPanel) {
        let w = batch.width;
        self.draw_lower_hotbar(batch, panel.band);
        let atlas = &self.gui.atlas;
        // The block the player stands in.
        let [x, y, sw, sh] = panel.coords;
        batch.fill(atlas, x, y, sw, sh, rgba(55, 55, 55, 255));
        if let Some(p) = &self.player {
            let at = [p.position.x, p.position.y, p.position.z].map(|v| v.floor() as i64);
            let text = format!("X: {}, Y: {}, Z: {}", at[0], at[1], at[2]);
            let tw = HudBatch::text_width(atlas, &text);
            batch.text(
                atlas,
                &text,
                ((w - tw) / 2.0).floor(),
                y + 2.0,
                u32::MAX,
                false,
            );
        }
        self.draw_panel_buttons(batch, panel);
    }

    /// The hotbar along the top, on a dark band `band` tall (`hotbar_with_attack_side` places
    /// it 22 pixels from the bottom of the height it is given).
    fn draw_lower_hotbar(&mut self, batch: &mut HudBatch, band: f32) {
        let w = batch.width;
        batch.fill(&self.gui.atlas, 0.0, 0.0, w, band, rgba(85, 85, 85, 255));
        let full = batch.height;
        batch.height = band - 1.0;
        let start = batch.vertices.len();
        self.draw_hotbar(batch);
        batch.height = full;
        // The off-hand slot (or the attack indicator) beside the hotbar can reach past the
        // screen's edges: the row shrinks and slides to fit, easing there over about 0.2 s.
        let (left, right) = batch.vertices[start..]
            .iter()
            .fold((f32::MAX, f32::MIN), |(l, r), v| {
                (l.min(v.pos[0]), r.max(v.pos[0]))
            });
        if left <= right {
            let target = hud::fit_row(left, right, w, 2.0);
            let (scale, dx, last) = FIT.get();
            let now = std::time::Instant::now();
            let k = last.map_or(1.0, |t| 1.0 - (-12.0 * (now - t).as_secs_f32()).exp());
            let (scale, dx) = (scale + (target.0 - scale) * k, dx + (target.1 - dx) * k);
            FIT.set((scale, dx, Some(now)));
            hud::transform_since(batch, start, [w / 2.0, HOTBAR_TOP], scale, [dx, 0.0]);
        }
    }

    /// The map and the inventory, crafting and pause buttons.
    fn draw_panel_buttons(&mut self, batch: &mut HudBatch, panel: &touch::DualPanel) {
        self.draw_panel_map(batch, panel.map);
        let atlas = &self.gui.atlas;
        for (i, [x, y, bw, bh]) in panel.buttons.into_iter().enumerate() {
            ui::nine_slice(
                batch,
                atlas,
                "gui/sprites/widget/button",
                x,
                y,
                bw,
                bh,
                u32::MAX,
            );
            let (cx, cy) = ((x + bw / 2.0).floor(), (y + bh / 2.0).floor());
            match i {
                0 | 1 => {
                    let icon = if i == 0 {
                        "minecraft:chest"
                    } else {
                        "minecraft:crafting_table"
                    };
                    let stack = ItemStack::new(icon, 1);
                    self.draw_item(batch, &stack, cx - 8.0, cy - 8.0);
                }
                // Pause: two bars.
                _ => {
                    let atlas = &self.gui.atlas;
                    for dx in [-4.0, 1.0] {
                        batch.fill(atlas, cx + dx, cy - 5.0, 3.0, 10.0, u32::MAX);
                    }
                }
            }
        }
    }

    /// The minimap image (the minimap extra's, which `minimap_wanted` asks for while the second
    /// screen is on) on a map item's paper, with the player's map marker in the middle.
    fn draw_panel_map(&self, batch: &mut HudBatch, [x, y, side, _]: [f32; 4]) {
        let atlas = &self.gui.atlas;
        if let Some(paper) = atlas.sprites.get("map/map_background").copied() {
            batch.rect_uv(x, y, side, side, paper, u32::MAX);
        }
        // `renderMap`'s paper reaches 7 pixels past the 128 of the map.
        let inset = (side * 7.0 / 142.0).round();
        let (mx, my, ms) = (x + inset, y + inset, side - 2.0 * inset);
        if !self.minimap_ready() {
            return;
        }
        if let Some(image) = atlas.sprites.get(MINIMAP_SPRITE).copied() {
            batch.rect_uv(mx, my, ms, ms, image, u32::MAX);
        }
        if let Some(r) = atlas.sprites.get("map/decorations/player").copied() {
            // As the minimap extra: the sprite's arrow points down the texture.
            let (s, c) = (self.yaw + 360.0).to_radians().sin_cos();
            let (cx, cy) = (mx + ms / 2.0, my + ms / 2.0);
            let p = |dx: f32, dy: f32| [cx + dx * c - dy * s, cy + dx * s + dy * c];
            let h = 4.0;
            batch.quad(
                [p(-h, h), p(h, h), p(h, -h), p(-h, -h)],
                [[r.u0, r.v0], [r.u1, r.v0], [r.u1, r.v1], [r.u0, r.v1]],
                u32::MAX,
            );
        }
    }
}
