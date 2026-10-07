use std::io::Read;

use folio_core::{Align, Block, Document, ListKind, ParaStyle, Paragraph, Run};

use super::*;

fn paras(doc: &Document) -> Vec<Paragraph> {
    doc.pages[0].doc().unwrap().blocks.iter().filter_map(|b| b.para().cloned()).collect()
}

fn find<'a>(ps: &'a [Paragraph], text: &str) -> &'a Paragraph {
    ps.iter().find(|p| p.text().contains(text)).unwrap_or_else(|| panic!("no paragraph with {text:?} in {:#?}", ps.iter().map(|p| p.text()).collect::<Vec<_>>()))
}

fn run<'a>(p: &'a Paragraph, text: &str) -> &'a Run {
    p.runs.iter().find(|r| r.text.contains(text)).unwrap_or_else(|| panic!("no run {text:?} in {:?}", p.runs))
}

#[test]
fn round_trip_keeps_text_styles_and_structure() {
    let doc = crate::docx::tests::rich();
    let (bytes, warnings) = export(&doc, &[0]).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    // mimetype first and stored.
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    {
        let f = z.by_index(0).unwrap();
        assert_eq!(f.name(), "mimetype");
        assert_eq!(f.compression(), zip::CompressionMethod::Stored);
    }
    for n in ["META-INF/manifest.xml", "content.xml", "styles.xml", "meta.xml"] {
        let mut s = String::new();
        z.by_name(n).unwrap().read_to_string(&mut s).unwrap();
        crate::docx::xml::parse(s.as_bytes()).unwrap_or_else(|e| panic!("{n}: {e}"));
    }

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

    let p = find(&ps, "Plain");
    assert_eq!(p.text(), "Plain, bold, italic, under struck red marked shaded big serif code E=mc2 H2O link tab\there.");
    assert!(run(p, "bold").style.bold);
    assert!(run(p, "italic").style.italic);
    assert!(run(p, "under").style.underline);
    assert!(run(p, "struck").style.strike);
    assert_eq!(run(p, "red").style.color.as_deref(), Some("#c00000"));
    assert_eq!(run(p, "shaded").style.highlight.as_deref(), Some("#ddeeff"));
    assert_eq!(run(p, "big").style.size, Some(16.0));
    assert_eq!(run(p, "serif").style.font.as_deref(), Some("serif"));
    assert!(run(p, "code").style.code);
    assert_eq!(run(p, "link").style.link.as_deref(), Some("https://example.com/a?b=1&c=2"));
    assert!(p.runs.iter().any(|r| r.text == "2" && r.style.superscript));
    assert!(p.runs.iter().any(|r| r.text == "2" && r.style.subscript));

    let p = find(&ps, "A claim");
    assert_eq!(p.text(), "A claim with a note. Commented words then addedremoved.");
    assert_eq!(run(p, "with a note").style.note.as_deref(), Some("Source: the archive."));
    assert_eq!(run(p, "added").style.inserted.as_deref(), Some("Grace"));
    assert_eq!(run(p, "removed").style.deleted.as_deref(), Some("Grace"));
    let cid = run(p, "Commented").style.comment.clone().expect("comment");
    assert_eq!(t.comments.len(), 1);
    assert_eq!(t.comments[0].id, cid);
    assert_eq!(t.comments[0].author, "Grace Hopper");
    assert!(t.comments[0].text.starts_with("Check this.\nSecond line."));
    assert!(t.comments[0].resolved);

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
    assert_eq!(table.rows[2][0].plain(), "Ink\nblack");
    assert_eq!(table.rows[1][1].fill.as_deref(), Some("#ffeeaa"));
    assert_eq!(table.rows[1][1].align, Align::Right);
    assert!((table.fractions()[0] - 0.75).abs() < 0.01);
    let img = t.blocks.iter().find_map(|b| if let Block::Image(i) = b { Some(i) } else { None }).unwrap();
    assert_eq!(img.caption, "Figure 1: the chart");
    assert_eq!(img.alt, "A chart");
    assert_eq!(img.align, Align::Center);
    assert!((img.width - 200.0).abs() < 0.1);
    assert_eq!(d.media[&img.media].bytes.as_slice(), crate::docx::tests::png().as_slice());
    assert!((t.setup.width - 612.0).abs() < 0.01);
    assert!((t.setup.margin_top - 72.0).abs() < 0.5, "{}", t.setup.margin_top);
    assert_eq!(t.setup.footer, "Page {page} of {pages}");
    assert!(t.track_changes);
}

const NS: &str = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:dc="http://purl.org/dc/elements/1.1/""#;

#[test]
fn reads_what_libreoffice_writes() {
    let content = format!(
        r##"<?xml version="1.0" encoding="UTF-8"?><office:document-content {NS} office:version="1.3"><office:automatic-styles>
        <style:style style:name="P1" style:family="paragraph" style:parent-style-name="Text_20_body"><style:paragraph-properties fo:text-align="center"/></style:style>
        <style:style style:name="P2" style:family="paragraph" style:parent-style-name="Text_20_body"><style:paragraph-properties fo:break-before="page"/></style:style>
        <style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold" fo:color="#ff0000"/></style:style>
        <text:list-style style:name="L1"><text:list-level-style-bullet text:level="1" text:bullet-char="•"/><text:list-level-style-number text:level="2" style:num-format="1"/></text:list-style>
        </office:automatic-styles><office:body><office:text>
        <text:h text:style-name="Heading_20_1" text:outline-level="1">Chapter</text:h>
        <text:h text:outline-level="3">Deep heading</text:h>
        <text:p text:style-name="P1">Centered  <text:span text:style-name="T1">bold red</text:span><text:s text:c="2"/>end<text:note text:note-class="footnote"><text:note-citation>1</text:note-citation><text:note-body><text:p>The note.</text:p></text:note-body></text:note></text:p>
        <text:p text:style-name="Text_20_body">Go to <text:a xlink:href="https://libreoffice.org/">the site</text:a>.<text:line-break/>Second line</text:p>
        <text:list text:style-name="L1"><text:list-item><text:p>bullet</text:p><text:list><text:list-item><text:p>numbered</text:p></text:list-item></text:list></text:list-item></text:list>
        <text:p text:style-name="P2">After break</text:p>
        <table:table table:name="T"><table:table-column table:number-columns-repeated="2"/><table:table-header-rows><table:table-row><table:table-cell><text:p>H1</text:p></table:table-cell><table:table-cell><text:p>H2</text:p></table:table-cell></table:table-row></table:table-header-rows>
        <table:table-row><table:table-cell table:number-columns-spanned="2"><text:p>span</text:p></table:table-cell><table:covered-table-cell/></table:table-row></table:table>
        <text:p>Quote me</text:p>
        </office:text></office:body></office:document-content>"##
    );
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-styles {NS}><office:styles>
        <style:style style:name="Standard" style:family="paragraph"/>
        <style:style style:name="Text_20_body" style:display-name="Text body" style:family="paragraph" style:parent-style-name="Standard"/>
        <style:style style:name="Heading_20_1" style:display-name="Heading 1" style:family="paragraph" style:default-outline-level="1"/>
        </office:styles><office:automatic-styles><style:page-layout style:name="pm1"><style:page-layout-properties fo:page-width="21.001cm" fo:page-height="29.7cm" fo:margin-top="2cm" fo:margin-bottom="2cm" fo:margin-left="2cm" fo:margin-right="2cm"/></style:page-layout></office:automatic-styles>
        <office:master-styles><style:master-page style:name="Standard" style:page-layout-name="pm1"><style:footer><text:p>Page <text:page-number>1</text:page-number> of <text:page-count>3</text:page-count></text:p></style:footer></style:master-page></office:master-styles></office:document-styles>"#
    );
    let mut z = crate::docx::xml::ZipOut::new();
    z.add_stored("mimetype", b"application/vnd.oasis.opendocument.text").unwrap();
    z.add("content.xml", content.as_bytes()).unwrap();
    z.add("styles.xml", styles.as_bytes()).unwrap();
    let got = import(&z.finish().unwrap(), "lo").unwrap();
    let ps = paras(&got.doc);
    assert_eq!(find(&ps, "Chapter").style, ParaStyle::Heading1);
    assert_eq!(find(&ps, "Deep heading").style, ParaStyle::Heading3);
    let p = find(&ps, "Centered");
    assert_eq!(p.align, Align::Center);
    assert_eq!(p.text(), "Centered bold red  end");
    assert!(run(p, "bold red").style.bold);
    assert_eq!(run(p, "bold red").style.color.as_deref(), Some("#ff0000"));
    assert_eq!(run(p, "end").style.note.as_deref(), Some("The note."));
    let p = find(&ps, "Go to");
    assert_eq!(p.text(), "Go to the site.");
    assert_eq!(run(p, "the site").style.link.as_deref(), Some("https://libreoffice.org/"));
    find(&ps, "Second line");
    let b = find(&ps, "bullet");
    assert_eq!((b.list, b.level), (Some(ListKind::Bullet), 0));
    let n = find(&ps, "numbered");
    assert_eq!((n.list, n.level), (Some(ListKind::Number), 1));
    let t = got.doc.pages[0].doc().unwrap();
    let i = t.blocks.iter().position(|b| b.plain() == "After break").unwrap();
    assert!(matches!(t.blocks[i - 1], Block::PageBreak { .. }));
    let table = t.blocks.iter().find_map(|b| if let Block::Table(t) = b { Some(t) } else { None }).unwrap();
    assert!(table.header);
    assert_eq!(table.rows[1][0].plain(), "span");
    assert!(got.warnings.iter().any(|w| w.contains("Merged")));
    assert_eq!(t.setup.footer, "Page {page} of {pages}");
    assert!((t.setup.margin_left - 56.69).abs() < 0.1);
}
