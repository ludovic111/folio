//! The faces folio sets text in, and the font system that shapes it.
//!
//! [`Fonts`] wraps a cosmic-text `FontSystem` whose database holds the bundled faces first
//! (IBM Plex Sans, Serif and Mono, Chakra Petch: OFL) and then the system's fonts, which are only
//! used for characters the bundled faces lack (CJK, emoji, other scripts). Latin text is always
//! set in the bundled faces, so a document lays out the same on every machine.
//!
//! Loading the system's fonts takes a while: share one instance with [`Fonts::shared`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use cosmic_text::fontdb;
use cosmic_text::{FontSystem, Style, Weight};
use folio_core::text::Family;

use crate::text::LayoutCache;

/// A face: a family and its weight and slant. The window maps it to its own font, the PDF
/// export to the font's bytes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Face {
    /// The family name as the font names itself ("IBM Plex Sans", "Chakra Petch", or a system
    /// family for fallback glyphs).
    pub family: String,
    /// The face's weight: 400 regular, 600 semibold, 700 bold (500 for Chakra Petch Medium).
    pub weight: u16,
    pub italic: bool,
}

/// The bundled font files (OFL), for the window to register and exports to embed.
pub fn bundled_fonts() -> Vec<&'static [u8]> {
    vec![
        include_bytes!("../fonts/ibmplexsans/IBMPlexSans-Regular.ttf"),
        include_bytes!("../fonts/ibmplexsans/IBMPlexSans-Italic.ttf"),
        include_bytes!("../fonts/ibmplexsans/IBMPlexSans-SemiBold.ttf"),
        include_bytes!("../fonts/ibmplexsans/IBMPlexSans-Bold.ttf"),
        include_bytes!("../fonts/ibmplexsans/IBMPlexSans-BoldItalic.ttf"),
        include_bytes!("../fonts/ibmplexserif/IBMPlexSerif-Regular.ttf"),
        include_bytes!("../fonts/ibmplexserif/IBMPlexSerif-Italic.ttf"),
        include_bytes!("../fonts/ibmplexserif/IBMPlexSerif-SemiBold.ttf"),
        include_bytes!("../fonts/ibmplexserif/IBMPlexSerif-Bold.ttf"),
        include_bytes!("../fonts/ibmplexserif/IBMPlexSerif-BoldItalic.ttf"),
        include_bytes!("../fonts/ibmplexmono/IBMPlexMono-Regular.ttf"),
        include_bytes!("../fonts/ibmplexmono/IBMPlexMono-Italic.ttf"),
        include_bytes!("../fonts/ibmplexmono/IBMPlexMono-SemiBold.ttf"),
        include_bytes!("../fonts/ibmplexmono/IBMPlexMono-Bold.ttf"),
        include_bytes!("../fonts/ibmplexmono/IBMPlexMono-BoldItalic.ttf"),
        include_bytes!("../fonts/chakrapetch/ChakraPetch-Regular.ttf"),
        include_bytes!("../fonts/chakrapetch/ChakraPetch-Medium.ttf"),
        include_bytes!("../fonts/chakrapetch/ChakraPetch-SemiBold.ttf"),
        include_bytes!("../fonts/chakrapetch/ChakraPetch-Bold.ttf"),
    ]
}

/// Vertical metrics of a face, as fractions of the size.
#[derive(Clone, Copy, Debug)]
pub(crate) struct VMetrics {
    pub ascent: f32,
    pub descent: f32,
}

/// The font system: bundled faces first, the system's for characters they don't have. Also
/// holds the paragraph layout cache, so re-laying out a document only reshapes what changed.
pub struct Fonts {
    pub(crate) sys: FontSystem,
    faces: HashMap<fontdb::ID, Face>,
    ids: HashMap<Face, fontdb::ID>,
    data: HashMap<fontdb::ID, (Arc<Vec<u8>>, u32)>,
    families: HashMap<String, Option<String>>,
    vmetrics: HashMap<(String, u16, bool), VMetrics>,
    pub(crate) cache: LayoutCache,
    pub(crate) swash: Option<cosmic_text::SwashCache>,
    /// The system's fonts are loaded the first time a character needs them.
    system: SystemFonts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SystemFonts {
    Never,
    Lazy,
    Loaded,
}

impl Default for Fonts {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Fonts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fonts").field("faces", &self.sys.db().len()).finish()
    }
}

fn locale() -> String {
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(var) {
            let v = v.split('.').next().unwrap_or("").replace('_', "-");
            if !v.is_empty() && v != "C" && v != "POSIX" {
                return v;
            }
        }
    }
    "en-US".into()
}

impl Fonts {
    /// The bundled faces, and the system's fonts the first time a character the bundled faces
    /// lack is laid out (scanning them takes a moment, so documents in Latin scripts never wait
    /// for it). Prefer [`Fonts::shared`].
    pub fn new() -> Self {
        Self::build(SystemFonts::Lazy)
    }

    /// The bundled faces and all the system's fonts, loaded now.
    pub fn with_system_fonts() -> Self {
        let mut f = Self::build(SystemFonts::Lazy);
        f.load_system_fonts();
        f
    }

    /// Only the bundled faces: fast and the same everywhere (tests, thumbnails). Characters
    /// they lack show as missing glyphs.
    pub fn bundled_only() -> Self {
        Self::build(SystemFonts::Never)
    }

    fn build(system: SystemFonts) -> Self {
        let mut db = fontdb::Database::new();
        for f in bundled_fonts() {
            db.load_font_source(fontdb::Source::Binary(Arc::new(f)));
        }
        db.set_sans_serif_family(Family::Sans.font_name());
        db.set_serif_family(Family::Serif.font_name());
        db.set_monospace_family(Family::Mono.font_name());
        let sys = FontSystem::new_with_locale_and_db(locale(), db);
        Fonts {
            sys,
            faces: HashMap::new(),
            ids: HashMap::new(),
            data: HashMap::new(),
            families: HashMap::new(),
            vmetrics: HashMap::new(),
            cache: LayoutCache::default(),
            swash: None,
            system,
        }
    }

    /// Loads the system's fonts if they may be loaded and aren't yet; whether anything changed.
    pub(crate) fn load_system_fonts(&mut self) -> bool {
        if self.system != SystemFonts::Lazy {
            return false;
        }
        self.system = SystemFonts::Loaded;
        let db = self.sys.db_mut();
        let before = db.len();
        db.load_system_fonts();
        let changed = db.len() > before;
        if changed {
            self.families.retain(|_, v| v.is_some());
            self.cache = LayoutCache::default();
        }
        changed
    }

    /// The process-wide font system (created on first use, with the system's fonts). Don't call
    /// it while already holding it (layout functions taking `&mut Fonts` never do).
    pub fn shared() -> MutexGuard<'static, Fonts> {
        static SHARED: OnceLock<Mutex<Fonts>> = OnceLock::new();
        SHARED.get_or_init(|| Mutex::new(Fonts::new())).lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The cosmic-text font system, for callers that shape their own text.
    pub fn font_system(&mut self) -> &mut FontSystem {
        &mut self.sys
    }

    /// Forgets every cached paragraph layout.
    pub fn clear_cache(&mut self) {
        self.cache = LayoutCache::default();
    }

    /// The family a run or style asks for, as a name the database knows: `sans`, `serif`,
    /// `mono`, `display`, a known alias, or an installed family's own name.
    pub fn family_name(&mut self, requested: &str) -> Option<String> {
        if let Some(f) = Family::parse(requested) {
            return Some(f.font_name().to_string());
        }
        let key = requested.trim().to_ascii_lowercase();
        if key.is_empty() {
            return None;
        }
        if let Some(v) = self.families.get(&key) {
            return v.clone();
        }
        let found = self.sys.db().faces().find_map(|f| f.families.iter().find(|(n, _)| n.eq_ignore_ascii_case(&key)).map(|(n, _)| n.clone()));
        self.families.insert(key, found.clone());
        found
    }

    /// The face behind a font id (cached).
    pub(crate) fn face_of(&mut self, id: fontdb::ID) -> Face {
        if let Some(f) = self.faces.get(&id) {
            return f.clone();
        }
        let face = match self.sys.db().face(id) {
            Some(info) => Face {
                family: info.families.iter().find(|(_, l)| *l == fontdb::Language::English_UnitedStates).or(info.families.first()).map(|(n, _)| n.clone()).unwrap_or_default(),
                weight: info.weight.0,
                italic: info.style != Style::Normal,
            },
            None => Face { family: Family::Sans.font_name().into(), weight: 400, italic: false },
        };
        self.faces.insert(id, face.clone());
        self.ids.entry(face.clone()).or_insert(id);
        face
    }

    /// The font id of a face (the closest match in the database).
    pub(crate) fn id_of(&mut self, face: &Face) -> Option<fontdb::ID> {
        if let Some(id) = self.ids.get(face) {
            return Some(*id);
        }
        let q = fontdb::Query {
            families: &[fontdb::Family::Name(&face.family)],
            weight: Weight(face.weight),
            stretch: fontdb::Stretch::Normal,
            style: if face.italic { Style::Italic } else { Style::Normal },
        };
        let id = self.sys.db().query(&q)?;
        self.ids.insert(face.clone(), id);
        Some(id)
    }

    /// A face's font file and its index in the file (collections), for embedding in exports.
    pub fn face_data(&mut self, face: &Face) -> Option<(Arc<Vec<u8>>, u32)> {
        let id = self.id_of(face)?;
        if let Some(d) = self.data.get(&id) {
            return Some(d.clone());
        }
        let d = self.sys.db().with_face_data(id, |bytes, index| (Arc::new(bytes.to_vec()), index))?;
        self.data.insert(id, d.clone());
        Some(d)
    }

    /// Ascent and descent of a face as fractions of the size.
    pub(crate) fn vmetrics(&mut self, family: &str, weight: u16, italic: bool) -> VMetrics {
        let key = (family.to_string(), weight, italic);
        if let Some(m) = self.vmetrics.get(&key) {
            return *m;
        }
        let face = Face { family: family.into(), weight, italic };
        let m = self
            .id_of(&face)
            .and_then(|id| self.sys.get_font(id, Weight(weight)))
            .map(|f| {
                let m = f.metrics();
                let upem = (m.units_per_em as f32).max(1.0);
                VMetrics { ascent: m.ascent / upem, descent: -m.descent / upem }
            })
            .filter(|m| m.ascent > 0.0)
            .unwrap_or(VMetrics { ascent: 1.025, descent: 0.275 });
        self.vmetrics.insert(key, m);
        m
    }

    /// Outline of a glyph at a size, in points with y down from the baseline (for rasterising).
    pub(crate) fn outline(&mut self, face: &Face, glyph: u16, size: f32) -> Option<Vec<cosmic_text::Command>> {
        let id = self.id_of(face)?;
        let (key, _, _) = cosmic_text::CacheKey::new(id, glyph, size, (0.0, 0.0), Weight(face.weight), cosmic_text::CacheKeyFlags::DISABLE_HINTING);
        let swash = self.swash.get_or_insert_with(cosmic_text::SwashCache::new);
        swash.get_outline_commands(&mut self.sys, key).map(|c| c.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_faces_resolve() {
        let mut f = Fonts::bundled_only();
        for fam in [Family::Sans, Family::Serif, Family::Mono, Family::Display] {
            let face = Face { family: fam.font_name().into(), weight: 400, italic: false };
            let (data, _) = f.face_data(&face).expect("bundled face");
            assert!(data.len() > 10_000);
        }
        let bold = Face { family: "IBM Plex Sans".into(), weight: 700, italic: true };
        let id = f.id_of(&bold).unwrap();
        assert_eq!(f.face_of(id), bold);
        assert_eq!(f.family_name("serif").as_deref(), Some("IBM Plex Serif"));
        assert_eq!(f.family_name("Chakra Petch").as_deref(), Some("Chakra Petch"));
        assert_eq!(f.family_name("No Such Font"), None);
        let m = f.vmetrics("IBM Plex Sans", 400, false);
        assert!(m.ascent > 0.8 && m.ascent < 1.2 && m.descent > 0.1, "{m:?}");
    }
}
