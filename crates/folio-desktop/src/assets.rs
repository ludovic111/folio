//! Bundled assets: lucide icons (ISC, `assets/icons/LICENSE.lucide.txt`), the logos of the apps
//! and services folio works with (`assets/logos/SOURCES.md`) and the fonts (OFL, in
//! folio-layout: Chakra Petch and IBM Plex Mono for the interface, IBM Plex Sans, Serif and Mono
//! for documents and slides).

use std::borrow::Cow;

use gpui::{App, AssetSource, SharedString};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "assets"]
#[include = "icons/*.svg"]
#[include = "logos/*.png"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(Self::get(path).map(|f| f.data))
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Self::iter().filter(|p| p.starts_with(path)).map(SharedString::from).collect())
    }
}

/// Registers the bundled faces (the same files the layout engine shapes with, so screen and
/// print agree).
pub fn load_fonts(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = folio_layout::bundled_fonts().into_iter().map(Cow::Borrowed).collect();
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        tracing::warn!("couldn't load the bundled fonts: {e}");
    }
}

/// An icon by lucide name, e.g. `icon_path("bold")`.
pub fn icon_path(name: &str) -> SharedString {
    format!("icons/{name}.svg").into()
}
