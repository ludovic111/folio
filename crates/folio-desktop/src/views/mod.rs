//! The window's views.

pub mod agent_panel;
pub mod deckview;
pub mod dialogs;
pub mod docview;
pub mod editor;
pub mod home;
pub mod inspector;
pub mod onboarding;
pub mod overlays;
pub mod present;
pub mod sheetview;

/// Runs `f` with the layout engine's shared fonts (the same faces the window paints with).
pub fn with_fonts<R>(f: impl FnOnce(&mut folio_layout::Fonts) -> R) -> R {
    f(&mut folio_layout::Fonts::shared())
}

/// Focuses a view (its focus handle), e.g. after a toolbar click.
pub fn focus<V: gpui::Focusable>(e: &gpui::Entity<V>, window: &mut gpui::Window, cx: &mut gpui::App) {
    let h = e.read(cx).focus_handle(cx);
    window.focus(&h, cx);
}
