use std::io::Read;

use folio_core::text::{Comment, ImageBlock, PageSetup, Reply, Table, TableCell};
use folio_core::{Align, Block, Document, Id, ListKind, PageKind, ParaStyle, Paragraph, Run, RunStyle};

use super::*;

/// A 4×2 PNG.
pub(crate) fn png() -> Vec<u8> {
    let img = image::RgbaImage::from_fn(4, 2, |x, _| if x % 2 == 0 { image::Rgba([200, 30, 30, 255]) } else { image::Rgba([20, 20, 200, 255]) });
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// A document with a bit of everything a word processor carries.
pub(crate) fn rich() -> Document {
    let mut doc = Document::new("Quarterly report");
    doc.meta.author = "Ada Lovelace".into();
    let media = doc.add_media("chart.png", png());
    let comment = Id::new();
    let t = doc.page_mut(0).doc_mut().unwrap();
    let style = |f: fn(&mut RunStyle)| {
        let mut s = RunStyle::default();
        f(&mut s);
        s
    };
    let mut rows = Table::from_text(vec![vec!["Item".into(), "Cost".into()], vec!["Paper".into(), "12".into()], vec!["Ink\nblack".into(), "30".into()]], true);
    rows.widths = vec![3.0, 1.0];
    rows.rows[1][1].fill = Some("#ffeeaa".into());
    rows.rows[1][1].align = Align::Right;
    t.blocks = imbl::vector![
        Block::Paragraph(Paragraph::new(ParaStyle::Title, "Quarterly report")),
        Block::Paragraph(Paragraph::new(ParaStyle::Subtitle, "Spring")),
        Block::Paragraph(Paragraph::new(ParaStyle::Heading1, "Summary")),
        Block::Paragraph(Paragraph::with_runs(
            ParaStyle::Normal,
            vec![
                Run::plain("Plain, "),
                Run::bold("bold"),
                Run::plain(", "),
                Run::italic("italic"),
                Run::styled(", under", style(|s| s.underline = true)),
                Run::styled(" struck", style(|s| s.strike = true)),
                Run::styled(" red", style(|s| s.color = Some("#c00000".into()))),
                Run::styled(" marked", style(|s| s.highlight = Some("#ffff00".into()))),
                Run::styled(" shaded", style(|s| s.highlight = Some("#ddeeff".into()))),
                Run::styled(" big", style(|s| s.size = Some(16.0))),
                Run::styled(" serif", style(|s| s.font = Some("serif".into()))),
                Run::styled(" code", style(|s| s.code = true)),
                Run::plain(" E=mc"),
                Run::styled("2", style(|s| s.superscript = true)),
                Run::plain(" H"),
                Run::styled("2", style(|s| s.subscript = true)),
                Run::plain("O "),
                Run::styled("link", style(|s| s.link = Some("https://example.com/a?b=1&c=2".into()))),
                Run::plain(" tab\there."),
            ],
        )),
        Block::Paragraph(Paragraph::with_runs(
            ParaStyle::Normal,
            vec![
                Run::plain("A claim"),
                Run::styled(" with a note", style(|s| s.note = Some("Source: the archive.".into()))),
                Run::plain(". "),
                Run { text: "Commented words".into(), style: RunStyle { comment: Some(comment.clone()), ..Default::default() } },
                Run::plain(" then "),
                Run::styled("added", style(|s| s.inserted = Some("Grace".into()))),
                Run::styled("removed", style(|s| s.deleted = Some("Grace".into()))),
                Run::plain("."),
            ],
        )),
        Block::Paragraph(Paragraph::new(ParaStyle::Heading2, "Lists")),
        Block::Paragraph(Paragraph::new(ParaStyle::Normal, "First bullet").list(ListKind::Bullet, 0)),
        Block::Paragraph(Paragraph::new(ParaStyle::Normal, "Nested number").list(ListKind::Number, 1)),
        Block::Paragraph(Paragraph::new(ParaStyle::Normal, "Second bullet").list(ListKind::Bullet, 0)),
        Block::Paragraph(Paragraph::new(ParaStyle::Normal, "To do").list(ListKind::Check, 0)),
        Block::Paragraph(Paragraph { checked: true, ..Paragraph::new(ParaStyle::Normal, "Done").list(ListKind::Check, 0) }),
        Block::Paragraph(Paragraph::new(ParaStyle::Normal, "One").list(ListKind::Number, 0)),
        Block::Paragraph(Paragraph::new(ParaStyle::Normal, "Two").list(ListKind::Number, 0)),
        Block::Paragraph(Paragraph::new(ParaStyle::Heading3, "Table")),
        Block::Table(rows),
        Block::Image(ImageBlock { id: Id::new(), media, width: 200.0, caption: "Figure 1: the chart".into(), alt: "A chart".into(), align: Align::Center }),
        Block::Paragraph(Paragraph::new(ParaStyle::Quote, "A quotation.").align(Align::Justify)),
        Block::Paragraph(Paragraph::new(ParaStyle::Code, "fn main() {}")),
        Block::PageBreak { id: Id::new() },
        Block::Paragraph(Paragraph::new(ParaStyle::Normal, "Centered after the break").align(Align::Center)),
        Block::Paragraph(Paragraph::new(ParaStyle::Caption, "A lone caption")),
    ];
    t.comments = vec![Comment {
        id: comment,
        author: "Grace Hopper".into(),
        text: "Check this.\nSecond line.".into(),
        at: chrono::DateTime::parse_from_rfc3339("2026-03-04T05:06:07Z").unwrap().with_timezone(&chrono::Utc),
        resolved: true,
        replies: vec![Reply { author: "Ada Lovelace".into(), text: "Checked.".into(), at: chrono::DateTime::parse_from_rfc3339("2026-03-05T05:06:07Z").unwrap().with_timezone(&chrono::Utc) }],
    }];
    t.setup = PageSetup { header: "{title}".into(), footer: "Page {page} of {pages}".into(), ..PageSetup::letter() };
    t.track_changes = true;
    doc
}

fn paras(doc: &Document) -> Vec<Paragraph> {
    doc.pages[0].doc().unwrap().blocks.iter().filter_map(|b| b.para().cloned()).collect()
}

fn find<'a>(ps: &'a [Paragraph], text: &str) -> &'a Paragraph {
    ps.iter().find(|p| p.text().contains(text)).unwrap_or_else(|| panic!("no paragraph with {text:?} in {:#?}", ps.iter().map(|p| p.text()).collect::<Vec<_>>()))
}

fn run<'a>(p: &'a Paragraph, text: &str) -> &'a Run {
    p.runs.iter().find(|r| r.text.contains(text)).unwrap_or_else(|| panic!("no run {text:?} in {:?}", p.runs))
}

pub(crate) fn part(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut s = String::new();
    z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}

#[test]
fn round_trip_keeps_text_styles_and_structure() {
    let doc = rich();
    let (bytes, warnings) = export(&doc, &[0]).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let back = import(&bytes, "Quarterly report").unwrap();
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    let d = back.doc;
    assert_eq!(d.meta.author, "Ada Lovelace");
    let t = d.pages[0].doc().unwrap();
    let kinds: Vec<&str> = t.blocks.iter().map(|b| b.kind()).collect();
    let want: Vec<&str> = doc.pages[0].doc().unwrap().blocks.iter().map(|b| b.kind()).collect();
    assert_eq!(kinds, want);

    let ps = paras(&d);
    assert_eq!(find(&ps, "Quarterly").style, ParaStyle::Title);
    assert_eq!(find(&ps, "Spring").style, ParaStyle::Subtitle);
    assert_eq!(find(&ps, "Summary").style, ParaStyle::Heading1);
    assert_eq!(find(&ps, "Lists").style, ParaStyle::Heading2);
    assert_eq!(find(&ps, "Table").style, ParaStyle::Heading3);
    assert_eq!(find(&ps, "quotation").style, ParaStyle::Quote);
    assert_eq!(find(&ps, "quotation").align, Align::Justify);
    assert_eq!(find(&ps, "fn main").style, ParaStyle::Code);
    assert_eq!(find(&ps, "Centered").align, Align::Center);
    assert_eq!(find(&ps, "lone caption").style, ParaStyle::Caption);

    let p = find(&ps, "Plain");
    assert_eq!(p.text(), "Plain, bold, italic, under struck red marked shaded big serif code E=mc2 H2O link tab\there.");
    assert!(run(p, "bold").style.bold);
    assert!(run(p, "italic").style.italic);
    assert!(run(p, "under").style.underline);
    assert!(run(p, "struck").style.strike);
    assert_eq!(run(p, "red").style.color.as_deref(), Some("#c00000"));
    assert_eq!(run(p, "marked").style.highlight.as_deref(), Some("#ffff00"));
    assert_eq!(run(p, "shaded").style.highlight.as_deref(), Some("#ddeeff"));
    assert_eq!(run(p, "big").style.size, Some(16.0));
    assert_eq!(run(p, "serif").style.font.as_deref(), Some("serif"));
    assert!(run(p, "code").style.code);
    assert_eq!(run(p, "link").style.link.as_deref(), Some("https://example.com/a?b=1&c=2"));
    assert!(p.runs.iter().any(|r| r.text == "2" && r.style.superscript));
    assert!(p.runs.iter().any(|r| r.text == "2" && r.style.subscript));
    assert_eq!(run(p, "Plain").style, RunStyle::default());

    let p = find(&ps, "A claim");
    assert_eq!(p.text(), "A claim with a note. Commented words then addedremoved.");
    assert_eq!(run(p, "with a note").style.note.as_deref(), Some("Source: the archive."));
    assert_eq!(run(p, "added").style.inserted.as_deref(), Some("Grace"));
    assert_eq!(run(p, "removed").style.deleted.as_deref(), Some("Grace"));
    let cid = run(p, "Commented").style.comment.clone().expect("comment");
    assert_eq!(t.comments.len(), 1);
    let c = &t.comments[0];
    assert_eq!(c.id, cid);
    assert_eq!(c.author, "Grace Hopper");
    assert_eq!(c.text, "Check this.\nSecond line.");
    assert!(c.resolved);
    assert_eq!(c.at.to_rfc3339(), "2026-03-04T05:06:07+00:00");
    assert_eq!(c.replies.len(), 1);
    assert_eq!(c.replies[0].text, "Checked.");

    let lists: Vec<(String, Option<ListKind>, u8, bool)> = ps.iter().filter(|p| p.list.is_some()).map(|p| (p.text(), p.list, p.level, p.checked)).collect();
    assert_eq!(
        lists,
        vec![
            ("First bullet".into(), Some(ListKind::Bullet), 0, false),
            ("Nested number".into(), Some(ListKind::Number), 1, false),
            ("Second bullet".into(), Some(ListKind::Bullet), 0, false),
            ("To do".into(), Some(ListKind::Check), 0, false),
            ("Done".into(), Some(ListKind::Check), 0, true),
            ("One".into(), Some(ListKind::Number), 0, false),
            ("Two".into(), Some(ListKind::Number), 0, false),
        ]
    );

    let table = t.blocks.iter().find_map(|b| if let Block::Table(t) = b { Some(t) } else { None }).unwrap();
    assert!(table.header);
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[2][0].plain(), "Ink\nblack");
    assert_eq!(table.rows[1][1].fill.as_deref(), Some("#ffeeaa"));
    assert_eq!(table.rows[1][1].align, Align::Right);
    assert!(table.rows[0][0].runs.iter().all(|r| !r.style.bold), "header bold is folio's");
    let f = table.fractions();
    assert!((f[0] - 0.75).abs() < 0.01, "{f:?}");

    let img = t.blocks.iter().find_map(|b| if let Block::Image(i) = b { Some(i) } else { None }).unwrap();
    assert_eq!(img.caption, "Figure 1: the chart");
    assert_eq!(img.alt, "A chart");
    assert_eq!(img.align, Align::Center);
    assert!((img.width - 200.0).abs() < 0.1, "{}", img.width);
    assert_eq!(d.media[&img.media].bytes.as_slice(), png().as_slice());

    assert_eq!(t.setup.width, 612.0);
    assert_eq!(t.setup.height, 792.0);
    assert_eq!(t.setup.margin_left, 72.0);
    assert_eq!(t.setup.footer, "Page {page} of {pages}");
    assert_eq!(t.setup.header, "Quarterly report");
    assert!(t.track_changes);
}

#[test]
fn package_is_well_formed() {
    let doc = rich();
    let (bytes, _) = export(&doc, &[0]).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let names: Vec<String> = z.file_names().map(str::to_string).collect();
    for want in ["[Content_Types].xml", "_rels/.rels", "word/document.xml", "word/styles.xml", "word/numbering.xml", "word/settings.xml", "word/footnotes.xml", "word/comments.xml", "word/_rels/document.xml.rels", "docProps/core.xml", "docProps/app.xml", "word/header1.xml", "word/footer1.xml"] {
        assert!(names.iter().any(|n| n == want), "missing {want} in {names:?}");
    }
    // Every part parses, and relationship ids are unique and all used targets exist.
    for n in &names {
        if n.ends_with(".xml") || n.ends_with(".rels") {
            let mut s = String::new();
            z.by_name(n).unwrap().read_to_string(&mut s).unwrap();
            xml::parse(s.as_bytes()).unwrap_or_else(|e| panic!("{n}: {e}"));
        }
    }
    let rels = xml::parse(part(&bytes, "word/_rels/document.xml.rels").as_bytes()).unwrap();
    let ids: Vec<&str> = rels.elements().filter_map(|r| r.attr("Id")).collect();
    let unique: std::collections::HashSet<&&str> = ids.iter().collect();
    assert_eq!(ids.len(), unique.len());
    for r in rels.elements() {
        if r.attr("TargetMode") != Some("External") {
            let target = format!("word/{}", r.attr("Target").unwrap());
            assert!(names.contains(&target), "{target} missing");
        }
    }
    let document = part(&bytes, "word/document.xml");
    for id in &ids {
        if document.contains(&format!("\"{id}\"")) {
            continue;
        }
    }
    assert!(document.contains("<w:footnoteReference w:id=\"1\"/>"));
    let ct = part(&bytes, "[Content_Types].xml");
    assert!(ct.contains("Extension=\"png\""));
}

/// Child elements in the order the WordprocessingML schema lists them.
#[test]
fn child_order_follows_the_schema() {
    const PPR: &[&str] = &["pStyle", "keepNext", "keepLines", "pageBreakBefore", "framePr", "widowControl", "numPr", "suppressLineNumbers", "pBdr", "shd", "tabs", "suppressAutoHyphens", "kinsoku", "wordWrap", "overflowPunct", "topLinePunct", "autoSpaceDE", "autoSpaceDN", "bidi", "adjustRightInd", "snapToGrid", "spacing", "ind", "contextualSpacing", "mirrorIndents", "suppressOverlap", "jc", "textDirection", "textAlignment", "textboxTightWrap", "outlineLvl", "divId", "cnfStyle", "rPr", "sectPr", "pPrChange"];
    const RPR: &[&str] = &["rStyle", "rFonts", "b", "bCs", "i", "iCs", "caps", "smallCaps", "strike", "dstrike", "outline", "shadow", "emboss", "imprint", "noProof", "snapToGrid", "vanish", "webHidden", "color", "spacing", "w", "kern", "position", "sz", "szCs", "highlight", "u", "effect", "bdr", "shd", "fitText", "vertAlign", "rtl", "cs", "em", "lang", "eastAsianLayout", "specVanish", "oMath"];
    const TBLPR: &[&str] = &["tblStyle", "tblpPr", "tblOverlap", "bidiVisual", "tblStyleRowBandSize", "tblStyleColBandSize", "tblW", "jc", "tblCellSpacing", "tblInd", "tblBorders", "shd", "tblLayout", "tblCellMar", "tblLook", "tblCaption", "tblDescription"];
    const TCPR: &[&str] = &["cnfStyle", "tcW", "gridSpan", "hMerge", "vMerge", "tcBorders", "shd", "noWrap", "tcMar", "textDirection", "tcFitText", "vAlign", "hideMark"];
    const SECTPR: &[&str] = &["headerReference", "footerReference", "footnotePr", "endnotePr", "type", "pgSz", "pgMar", "paperSrc", "pgBorders", "lnNumType", "pgNumType", "cols", "formProt", "vAlign", "noEndnote", "titlePg", "textDirection", "bidi", "rtlGutter", "docGrid", "printerSettings", "sectPrChange"];
    const STYLE: &[&str] = &["name", "aliases", "basedOn", "next", "link", "autoRedefine", "hidden", "uiPriority", "semiHidden", "unhideWhenUsed", "qFormat", "locked", "personal", "personalCompose", "personalReply", "rsid", "pPr", "rPr", "tblPr", "trPr", "tcPr", "tblStylePr"];
    const LVL: &[&str] = &["start", "numFmt", "lvlRestart", "pStyle", "isLgl", "suff", "lvlText", "lvlPicBulletId", "legacy", "lvlJc", "pPr", "rPr"];
    const SETTINGS: &[&str] = &["writeProtection", "view", "zoom", "removePersonalInformation", "removeDateAndTime", "doNotDisplayPageBoundaries", "displayBackgroundShape", "printPostScriptOverText", "printFractionalCharacterWidth", "printFormsData", "embedTrueTypeFonts", "embedSystemFonts", "saveSubsetFonts", "saveFormsData", "mirrorMargins", "alignBordersAndEdges", "bordersDoNotSurroundHeader", "bordersDoNotSurroundFooter", "gutterAtTop", "hideSpellingErrors", "hideGrammaticalErrors", "activeWritingStyle", "proofState", "formsDesign", "attachedTemplate", "linkStyles", "stylePaneFormatFilter", "stylePaneSortMethod", "documentType", "mailMerge", "revisionView", "trackRevisions", "doNotTrackMoves", "doNotTrackFormatting", "documentProtection", "autoFormatOverride", "styleLockTheme", "styleLockQFSet", "defaultTabStop", "autoHyphenation", "consecutiveHyphenLimit", "hyphenationZone", "doNotHyphenateCaps", "showEnvelope", "summaryLength", "clickAndTypeStyle", "defaultTableStyle", "evenAndOddHeaders", "bookFoldRevPrinting", "bookFoldPrinting", "bookFoldPrintingSheets", "drawingGridHorizontalSpacing", "drawingGridVerticalSpacing", "displayHorizontalDrawingGridEvery", "displayVerticalDrawingGridEvery", "doNotUseMarginsForDrawingGridOrigin", "drawingGridHorizontalOrigin", "drawingGridVerticalOrigin", "doNotShadeFormData", "noPunctuationKerning", "characterSpacingControl", "printTwoOnOne", "strictFirstAndLastChars", "noLineBreaksAfter", "noLineBreaksBefore", "savePreviewPicture", "doNotValidateAgainstSchema", "saveInvalidXml", "ignoreMixedContent", "alwaysShowPlaceholderText", "doNotDemarcateInvalidXml", "saveXmlDataOnly", "useXSLTWhenSaving", "saveThroughXslt", "showXMLTags", "alwaysMergeEmptyNamespace", "updateFields", "hdrShapeDefaults", "footnotePr", "endnotePr", "compat", "docVars", "rsids", "mathPr", "attachedSchema", "themeFontLang", "clrSchemeMapping", "doNotIncludeSubdocsInStats", "doNotAutoCompressPictures", "forceUpgrade", "captions", "readModeInkLockDown", "smartTagType", "schemaLibrary", "shapeDefaults", "doNotEmbedSmartTags", "decimalSymbol", "listSeparator"];
    fn check(el: &xml::El, order: &[(&str, &[&str])]) {
        for (name, seq) in order {
            if el.name == format!("w:{name}") {
                let mut last = 0;
                for k in el.elements() {
                    let local = k.local();
                    let pos = seq.iter().position(|s| *s == local).unwrap_or_else(|| panic!("<{}> isn't allowed in <w:{name}>", k.name));
                    assert!(pos >= last, "<{}> comes too late in <w:{name}>: {:?}", k.name, el.elements().map(|e| e.name.clone()).collect::<Vec<_>>());
                    last = pos;
                }
            }
        }
        for k in el.elements() {
            check(k, order);
        }
    }
    let order: &[(&str, &[&str])] = &[("pPr", PPR), ("rPr", RPR), ("tblPr", TBLPR), ("tcPr", TCPR), ("sectPr", SECTPR), ("style", STYLE), ("lvl", LVL), ("settings", SETTINGS)];
    let (bytes, _) = export(&rich(), &[0]).unwrap();
    for name in ["word/document.xml", "word/styles.xml", "word/numbering.xml", "word/settings.xml", "word/footnotes.xml", "word/comments.xml"] {
        let root = xml::parse(part(&bytes, name).as_bytes()).unwrap();
        check(&root, order);
    }
}

#[test]
fn several_pages_become_sections_and_sheets_tables() {
    let mut doc = rich();
    let i = doc.add_page(PageKind::Doc, Some("Annex"), None).unwrap();
    let t = doc.page_mut(i).doc_mut().unwrap();
    t.blocks = imbl::vector![Block::Paragraph(Paragraph::new(ParaStyle::Heading1, "Annex"))];
    t.setup = PageSetup { width: 842.0, height: 595.0, ..PageSetup::a4() };
    let s = doc.add_page(PageKind::Sheet, Some("Budget"), None).unwrap();
    let sh = doc.page_mut(s).sheet_mut().unwrap();
    sh.set_input(folio_calc::Addr::new(0, 0), "Month");
    sh.set_input(folio_calc::Addr::new(1, 0), "Jan");
    sh.set_input(folio_calc::Addr::new(1, 1), "12");
    doc.add_page(PageKind::Deck, Some("Pitch"), None).unwrap();
    let all: Vec<usize> = (0..doc.pages.len()).collect();
    let (bytes, warnings) = export(&doc, &all).unwrap();
    assert!(warnings.iter().any(|w| w.contains("Pitch") && w.contains("deck")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("Budget") && w.contains("table")), "{warnings:?}");
    let document = part(&bytes, "word/document.xml");
    assert_eq!(document.matches("<w:sectPr>").count(), 3);
    assert!(document.contains("w:orient=\"landscape\""));
    let back = import(&bytes, "x").unwrap();
    let t = back.doc.pages[0].doc().unwrap();
    assert!(t.blocks.iter().any(|b| matches!(b, Block::Paragraph(p) if p.text() == "Annex")));
    assert!(t.blocks.iter().any(|b| matches!(b, Block::Table(t) if t.rows.iter().any(|r| r.iter().any(|c| c.plain() == "Jan")))));
    assert!(back.warnings.iter().any(|w| w.contains("sections")), "{:?}", back.warnings);
}

/// Minimal packages in the shapes other apps write.
fn package(document: &str, extra: &[(&str, &str)], rels: &str) -> Vec<u8> {
    let mut z = xml::ZipOut::new();
    z.add("[Content_Types].xml", br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#).unwrap();
    z.add("_rels/.rels", br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#).unwrap();
    z.add("word/document.xml", document.as_bytes()).unwrap();
    z.add("word/_rels/document.xml.rels", format!(r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#).as_bytes()).unwrap();
    for (n, b) in extra {
        z.add(n, b.as_bytes()).unwrap();
    }
    z.finish().unwrap()
}

const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;

#[test]
fn reads_what_word_and_google_docs_write() {
    // Styles by name (a localized id), a basedOn chain, outline levels, fields, content
    // controls, tracked changes, line and page breaks, a hyperlink field, symbols.
    let styles = format!(
        r#"<w:styles {W}>
        <w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/><w:sz w:val="22"/></w:rPr></w:rPrDefault></w:docDefaults>
        <w:style w:type="paragraph" w:default="1" w:styleId="Standard"><w:name w:val="Normal"/></w:style>
        <w:style w:type="paragraph" w:styleId="berschrift1"><w:name w:val="heading 1"/><w:basedOn w:val="Standard"/><w:rPr><w:b/><w:sz w:val="32"/><w:color w:val="2F5496"/></w:rPr></w:style>
        <w:style w:type="paragraph" w:styleId="MyHeading"><w:name w:val="My Heading"/><w:basedOn w:val="berschrift1"/></w:style>
        <w:style w:type="paragraph" w:styleId="Outline2"><w:name w:val="Outline two"/><w:pPr><w:outlineLvl w:val="1"/></w:pPr></w:style>
        <w:style w:type="paragraph" w:styleId="Heading5"><w:name w:val="heading 5"/></w:style>
        <w:style w:type="paragraph" w:styleId="Loud"><w:name w:val="Loud"/><w:rPr><w:b/><w:sz w:val="28"/></w:rPr></w:style>
        <w:style w:type="character" w:styleId="Strong"><w:name w:val="Strong"/><w:rPr><w:b/></w:rPr></w:style>
        <w:style w:type="character" w:styleId="HTMLCode"><w:name w:val="HTML Code"/><w:rPr><w:rFonts w:ascii="Courier New"/></w:rPr></w:style>
        </w:styles>"#
    );
    let numbering = format!(
        r#"<w:numbering {W}>
        <w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val="☐"/></w:lvl></w:abstractNum>
        <w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl><w:lvl w:ilvl="1"><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%2)"/></w:lvl></w:abstractNum>
        <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
        <w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>
        </w:numbering>"#
    );
    let body = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {W}><w:body>
        <w:p><w:pPr><w:pStyle w:val="berschrift1"/></w:pPr><w:r><w:rPr><w:b/><w:sz w:val="32"/></w:rPr><w:t>Kapitel</w:t></w:r></w:p>
        <w:p><w:pPr><w:pStyle w:val="MyHeading"/></w:pPr><w:r><w:t>Derived</w:t></w:r></w:p>
        <w:p><w:pPr><w:pStyle w:val="Outline2"/></w:pPr><w:r><w:t>By outline</w:t></w:r></w:p>
        <w:p><w:pPr><w:pStyle w:val="Heading5"/></w:pPr><w:r><w:t>Deep</w:t></w:r></w:p>
        <w:p><w:pPr><w:pStyle w:val="Loud"/></w:pPr><w:r><w:t>Custom loud</w:t></w:r></w:p>
        <w:p><w:r><w:rPr><w:rStyle w:val="Strong"/></w:rPr><w:t>strong</w:t></w:r><w:r><w:t xml:space="preserve"> and </w:t></w:r><w:r><w:rPr><w:rStyle w:val="HTMLCode"/></w:rPr><w:t>code()</w:t></w:r><w:r><w:rPr><w:rFonts w:ascii="Georgia"/></w:rPr><w:t xml:space="preserve"> georgia</w:t></w:r></w:p>
        <w:p><w:r><w:t>Page </w:t></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>7</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:t xml:space="preserve"> </w:t></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> HYPERLINK "https://folio.example/" </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>site</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>
        <w:sdt><w:sdtPr><w:alias w:val="Box"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>In a control</w:t></w:r></w:p></w:sdtContent></w:sdt>
        <w:p><w:ins w:id="1" w:author="Ann"><w:r><w:t>new</w:t></w:r></w:ins><w:del w:id="2" w:author="Bob"><w:r><w:delText>old</w:delText></w:r></w:del></w:p>
        <w:p><w:r><w:t>line one</w:t><w:br/><w:t>line two</w:t><w:br w:type="page"/><w:t>next page</w:t></w:r></w:p>
        <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Google check</w:t></w:r></w:p>
        <w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="2"/></w:numPr></w:pPr><w:r><w:t>sub item</w:t></w:r></w:p>
        <w:p><w:r><w:sym w:font="Symbol" w:char="F0B7"/><w:t xml:space="preserve"> sym</w:t></w:r><w:proofErr w:type="spellStart"/><w:bookmarkStart w:id="0" w:name="x"/><w:r><w:rPr><w:vanish/></w:rPr><w:t>hidden</w:t></w:r></w:p>
        <w:tbl><w:tblPr><w:tblLook w:val="0000"/></w:tblPr><w:tblGrid><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/><w:gridCol w:w="2000"/></w:tblGrid>
          <w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>wide</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>c</w:t></w:r></w:p><w:p><w:r><w:t>d</w:t></w:r></w:p></w:tc></w:tr>
          <w:tr><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:shd w:val="clear" w:color="auto" w:fill="FF0000"/></w:tcPr><w:p><w:r><w:t>3</w:t></w:r></w:p></w:tc></w:tr>
        </w:tbl>
        <w:p/>
        <w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1080" w:bottom="1440" w:left="1080" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr>
        </w:body></w:document>"#
    );
    let rels = r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>"#;
    let bytes = package(&body, &[("word/styles.xml", &styles), ("word/numbering.xml", &numbering)], rels);
    let got = import(&bytes, "Sample").unwrap();
    let ps = paras(&got.doc);
    assert_eq!(find(&ps, "Kapitel").style, ParaStyle::Heading1);
    assert!(find(&ps, "Kapitel").runs.iter().all(|r| r.style == RunStyle::default()), "{:?}", find(&ps, "Kapitel").runs);
    assert_eq!(find(&ps, "Derived").style, ParaStyle::Heading1);
    assert_eq!(find(&ps, "By outline").style, ParaStyle::Heading2);
    assert_eq!(find(&ps, "Deep").style, ParaStyle::Heading3);
    let loud = find(&ps, "Custom loud");
    assert_eq!(loud.style, ParaStyle::Normal);
    assert!(loud.runs[0].style.bold && loud.runs[0].style.size == Some(14.0));
    let p = find(&ps, "strong");
    assert!(run(p, "strong").style.bold);
    assert!(run(p, "code()").style.code);
    assert_eq!(run(p, "georgia").style.font.as_deref(), Some("serif"));
    let p = find(&ps, "Page");
    assert_eq!(p.text(), "Page 7 site");
    assert_eq!(run(p, "site").style.link.as_deref(), Some("https://folio.example/"));
    find(&ps, "In a control");
    let p = find(&ps, "new");
    assert_eq!(run(p, "new").style.inserted.as_deref(), Some("Ann"));
    assert_eq!(run(p, "old").style.deleted.as_deref(), Some("Bob"));
    assert_eq!(find(&ps, "line one").text(), "line one");
    assert_eq!(find(&ps, "line two").text(), "line two");
    let t = got.doc.pages[0].doc().unwrap();
    let pos_two = t.blocks.iter().position(|b| b.plain() == "line two").unwrap();
    assert!(matches!(t.blocks[pos_two + 1], Block::PageBreak { .. }));
    assert_eq!(t.blocks[pos_two + 2].plain(), "next page");
    let g = find(&ps, "Google check");
    assert_eq!((g.list, g.checked), (Some(ListKind::Check), false));
    let s = find(&ps, "sub item");
    assert_eq!((s.list, s.level), (Some(ListKind::Number), 1));
    assert_eq!(find(&ps, "sym").text(), "• sym");
    assert!(got.warnings.iter().any(|w| w.contains("Hidden")), "{:?}", got.warnings);
    let table = t.blocks.iter().find_map(|b| if let Block::Table(t) = b { Some(t) } else { None }).unwrap();
    assert_eq!(table.rows[0].iter().map(TableCell::plain).collect::<Vec<_>>(), ["wide", "", "c\nd"]);
    assert_eq!(table.rows[1][2].fill.as_deref(), Some("#ff0000"));
    assert!(!table.header);
    assert!(got.warnings.iter().any(|w| w.contains("Merged")), "{:?}", got.warnings);
    assert_eq!(t.setup.margin_left, 54.0);
    assert_eq!(t.setup.size_name(), "a4");
    assert_eq!(t.setup.footer, "");
}

#[test]
fn strict_namespaces_and_odd_prefixes_read_the_same() {
    let body = r#"<?xml version="1.0"?><x:document xmlns:x="http://purl.oclc.org/ooxml/wordprocessingml/main"><x:body><x:p><x:r><x:rPr><x:i/></x:rPr><x:t>strict</x:t></x:r></x:p></x:body></x:document>"#;
    let bytes = package(body, &[], "");
    let got = import(&bytes, "s").unwrap();
    let ps = paras(&got.doc);
    assert!(run(find(&ps, "strict"), "strict").style.italic);
}

#[test]
fn not_a_docx() {
    assert!(import(b"hello", "x").unwrap_err().contains("zip"));
    let mut z = xml::ZipOut::new();
    z.add("a.txt", b"x").unwrap();
    assert!(import(&z.finish().unwrap(), "x").unwrap_err().contains("word/document.xml"));
}

/// Writes sample exports to `$FOLIO_IO_SAMPLES` for checks outside Rust (the OOXML schema
/// validator, LibreOffice). Does nothing without the variable.
#[test]
fn write_samples_for_external_checks() {
    let Ok(dir) = std::env::var("FOLIO_IO_SAMPLES") else { return };
    let dir = std::path::Path::new(&dir);
    std::fs::create_dir_all(dir).unwrap();
    let mut doc = rich();
    let s = doc.add_page(PageKind::Sheet, Some("Budget"), None).unwrap();
    let sh = doc.page_mut(s).sheet_mut().unwrap();
    for (r, (m, v, c)) in [("Month", "Sales", "Costs"), ("Jan", "10", "4"), ("Feb", "14", "6"), ("Mar", "9", "5")].iter().enumerate() {
        sh.set_input(folio_calc::Addr::new(r as u32, 0), m);
        sh.set_input(folio_calc::Addr::new(r as u32, 1), v);
        sh.set_input(folio_calc::Addr::new(r as u32, 2), c);
    }
    let mut ed = folio_core::Editor::new(doc);
    let _ = &mut ed;
    let mut doc = ed.doc().clone();
    let t = doc.page_mut(0).doc_mut().unwrap();
    let chart = folio_core::Chart::new(folio_core::ChartKind::Column, "'Budget'!A1:C4");
    t.blocks.push_back(Block::Chart(folio_core::text::ChartBlock { id: Id::new(), chart, height: 200.0 }));
    let mut linked = Table::new(1, 1);
    linked.link = Some("'Budget'!A1:C4".into());
    t.blocks.push_back(Block::Table(linked));
    doc.add_page(PageKind::Deck, Some("Pitch"), None).unwrap();
    let all: Vec<usize> = (0..doc.pages.len()).collect();
    for (name, bytes) in [
        ("rich.docx", export(&rich(), &[0]).unwrap().0),
        ("all.docx", export(&doc, &all).unwrap().0),
        ("rich.odt", crate::odt::export(&rich(), &[0]).unwrap().0),
        ("all.odt", crate::odt::export(&doc, &all).unwrap().0),
        ("all.html", crate::html::export(&doc, &all).unwrap().0),
        ("rich.html", crate::html::export(&rich(), &[0]).unwrap().0),
    ] {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
}
