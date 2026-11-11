//! `Hud.extractItemHotbar`: on the window, or on a dual-screen handheld's second screen
//! (`second_screen.rs`), at the bottom of whichever `batch` it is given.
use super::*;

/// A dual-screen handheld's second screen is drawn on (the Android build's `second_screen.rs`).
pub(super) fn dual_screen() -> bool {
    #[cfg(target_os = "android")]
    return super::second_screen::active();
    #[cfg(not(target_os = "android"))]
    false
}

impl Game<'_> {
    /// The open screen draws on the second screen, as New 3DS Edition's menus open on its
    /// touch screen; the window keeps the world. Not chat (typed over the world), the game
    /// mode switcher (an overlay), the end poem and credits (watched), or the pause without
    /// its menu (F3+Esc).
    pub(super) fn screen_on_second_screen(&self) -> bool {
        if !dual_screen() {
            return false;
        }
        // At the title: its screens besides the main one (whose buttons the second screen draws
        // itself), and Options or a mod's page opened from it. The window keeps the logo.
        if let Some(title) = &self.title {
            return matches!(self.screen, Screen::Options | Screen::Mod)
                || !matches!(title.mode, horizon_client::title::Mode::Main);
        }
        !matches!(
            self.screen,
            Screen::Playing | Screen::Chat | Screen::GameModeSwitcher | Screen::Win
        ) && (self.screen != Screen::Paused || self.pause_menu)
    }

    pub(super) fn draw_hotbar(&mut self, batch: &mut HudBatch) {
        if self.spectator() {
            self.draw_spectator_hotbar(batch);
            return;
        }
        let (wi, hi) = (batch.width as i32, batch.height as i32);
        let indicator = self.settings.attack_indicator;
        #[cfg(target_os = "android")]
        let indicator =
            horizon_client::touch::attack_indicator(indicator, self.interaction_crosshair());
        let strength = self.hand_strength.scale(0.0, self.main_hand_attack_speed());
        let partial = self.clock.fraction() as f32;
        let inputs = hud_layout::HotbarInputs {
            selected: self.selected as i32,
            right_handed: !self.settings.main_hand_left,
            offhand: self.offhand.is_some(),
            slots: std::array::from_fn(|i| {
                self.inventory[i]
                    .as_ref()
                    .map(|_| (f32::from(self.hotbar_pop[i]) - partial).max(0.0))
            }),
            offhand_pop: (f32::from(self.hotbar_pop[9]) - partial).max(0.0),
            attack: (indicator == 2).then_some(strength),
        };
        #[cfg(target_os = "android")]
        let attack_left = Some(self.settings.touch.attack_indicator_left);
        #[cfg(not(target_os = "android"))]
        let attack_left = None;
        let draws = hud_layout::hotbar_with_attack_side(wi, hi, &inputs, attack_left);
        hud_layout::paint(batch, &self.gui.atlas, &draws);
        for d in &draws {
            if let hud_layout::GuiDraw::Item { slot, x, y, .. } = *d {
                let stack = if slot == 40 {
                    self.offhand.as_ref()
                } else {
                    self.inventory[slot as usize].as_ref()
                };
                if let Some(stack) = stack {
                    self.draw_item(batch, stack, x as f32, y as f32);
                }
            }
        }
    }
}
