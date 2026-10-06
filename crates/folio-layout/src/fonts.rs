//! The faces folio sets text in, and the font system that shapes it.
//!
//! PLACEHOLDER: the real cosmic-text implementation replaces `layout` (see the crate docs).

use folio_core::Paragraph;

use crate::text::{Glyph, Line, ParaCtx, ParaLayout};

/// A face: a family and its weight and slant. The window maps it to its own font, the PDF
/// export to the font's bytes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Face {
    pub family: String,
    /// 400 regular, 600 semibold, 700 bold.
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

/// The font system: bundled faces first, the system's for characters they don't have.
#[derive(Default)]
pub struct Fonts {}

impl Fonts {
    pub fn new() -> Self {
        Fonts {}
    }
}

/// Placeholder layout: one line per paragraph, fixed advances.
pub(crate) fn layout(_fonts: &mut Fonts, p: &Paragraph, width: f32, ctx: &ParaCtx) -> ParaLayout {
    let spec = p.style.spec();
    let size = spec.size * ctx.scale;
    let lh = size * spec.line;
    let text: Vec<char> = p.text().chars().collect();
    let adv = size * 0.55;
    let per_line = ((width / adv).floor() as usize).max(1);
    let mut lines = vec![];
    let mut start = 0;
    loop {
        let end = (start + per_line).min(text.len());
        let y = lines.len() as f32 * lh;
        let carets = (start..=end).map(|o| (o, (o - start) as f32 * adv)).collect();
        let glyphs = (start..end).map(|i| Glyph { face: 0, id: 0, x: (i - start) as f32 * adv, y: y + size, size, color: ctx.color }).collect();
        lines.push(Line { y, height: lh, baseline: y + size, start, end, glyphs, decos: vec![], carets });
        if end >= text.len() {
            break;
        }
        start = end;
    }
    let height = lines.len() as f32 * lh;
    ParaLayout { lines, height, space_before: spec.space_before * ctx.scale, space_after: spec.space_after * ctx.scale, indent: 0.0, marker: None, faces: vec![Face { family: spec.family.font_name().into(), weight: 400, italic: false }], notes: vec![], width }
}
