//! The logos of the apps and services folio works with (the office suites whose files it opens,
//! AI providers, the other lsuite apps), shown next to their names. The files are the makers'
//! own (`assets/logos/SOURCES.md` says where each came from); they are full colour, so they are
//! drawn with `img()`, not tinted like icons.

use gpui::{App, IntoElement, ObjectFit, Pixels, RenderOnce, Styled, Window, div, img, prelude::*};

use crate::theme::ActiveTheme;
use crate::ui::icon;

/// A logo file in `assets/logos/`, and whether it has a `-dark` twin for the dark theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogoFile {
    pub file: &'static str,
    pub dark: bool,
}

const fn one(file: &'static str) -> Option<LogoFile> {
    Some(LogoFile { file, dark: false })
}

const fn themed(file: &'static str) -> Option<LogoFile> {
    Some(LogoFile { file, dark: true })
}

/// Every id the window can name, with its logo; `None` shows a generic icon.
pub const LOGOS: &[(&str, Option<LogoFile>)] = &[
    ("word", one("word")),
    ("excel", one("excel")),
    ("powerpoint", one("powerpoint")),
    ("google-docs", one("google-docs")),
    ("google-sheets", one("google-sheets")),
    ("google-slides", one("google-slides")),
    ("pages", one("pages")),
    ("numbers", one("numbers")),
    ("keynote", one("keynote")),
    ("libreoffice-writer", one("libreoffice-writer")),
    ("libreoffice-calc", one("libreoffice-calc")),
    ("libreoffice-impress", one("libreoffice-impress")),
    // Agent providers.
    ("lsuite", one("lsuite")),
    ("claude", one("claude")),
    ("claude-code", one("claude")),
    ("anthropic", one("claude")),
    ("openai", themed("openai")),
    ("codex", themed("openai")),
    ("gemini", one("gemini")),
    ("ollama", themed("ollama")),
    ("mistral", one("mistral")),
    ("openrouter", one("openrouter")),
    ("lmstudio", None),
    ("openai-compatible", None),
    // lsuite apps.
    ("kimchi", one("kimchi")),
    ("nori", one("nori")),
];

pub fn logo_file(id: &str) -> Option<LogoFile> {
    let id = id.trim().to_ascii_lowercase();
    LOGOS.iter().find(|(k, _)| *k == id).and_then(|(_, f)| *f)
}

pub fn logo_path(f: LogoFile, dark: bool) -> String {
    if dark && f.dark { format!("logos/{}-dark.png", f.file) } else { format!("logos/{}.png", f.file) }
}

/// A logo, `size` square, picked for the theme; ids without one show a generic icon.
pub fn logo(id: &str, size: Pixels) -> Logo {
    Logo { file: logo_file(id), size }
}

#[derive(IntoElement)]
pub struct Logo {
    file: Option<LogoFile>,
    size: Pixels,
}

impl RenderOnce for Logo {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let size = self.size;
        match self.file {
            Some(f) => div().flex_none().size(size).child(img(logo_path(f, cx.theme().is_dark())).size(size).object_fit(ObjectFit::Contain)),
            None => div().flex_none().size(size).flex().items_center().justify_center().child(icon("box").size(size * 0.9)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::Assets;

    #[test]
    fn every_logo_is_bundled_listed_and_sourced() {
        let sources = include_str!("../../assets/logos/SOURCES.md");
        for (id, f) in LOGOS {
            if let Some(f) = f {
                for dark in [false, true] {
                    let path = logo_path(*f, dark);
                    let data = Assets::get(&path).unwrap_or_else(|| panic!("`{id}`: {path} isn't bundled"));
                    image::load_from_memory(&data.data).unwrap_or_else(|e| panic!("`{id}`: {path}: {e}"));
                    assert!(sources.contains(&format!("`{}`", path.trim_start_matches("logos/"))), "{path} isn't in SOURCES.md");
                }
            }
        }
    }
}
