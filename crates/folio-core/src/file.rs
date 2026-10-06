//! `.folio` files: a zip holding `mimetype` (stored first, like ODF), `document.json` (the
//! [`Document`] as JSON) and `media/<id>.<ext>` for each image. See `docs/FILE_FORMAT.md`.

use std::io::{Cursor, Read, Write};
use std::path::Path;
use std::sync::Arc;

use crate::{Document, Error, Result};

pub const MIME: &str = "application/vnd.lsuite.folio";
pub const EXTENSION: &str = "folio";

fn err(e: impl std::fmt::Display) -> Error {
    Error(e.to_string())
}

/// The file's bytes.
pub fn to_bytes(doc: &Document) -> Result<Vec<u8>> {
    let mut doc_json = doc.clone();
    doc_json.meta.generator = format!("folio {}", env!("CARGO_PKG_VERSION"));
    let json = serde_json::to_vec_pretty(&doc_json).map_err(err)?;
    let mut buf = Cursor::new(Vec::with_capacity(json.len() / 3 + doc.media.values().map(|m| m.bytes.len()).sum::<usize>()));
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        z.start_file("mimetype", stored).map_err(err)?;
        z.write_all(MIME.as_bytes()).map_err(err)?;
        z.start_file("document.json", deflated).map_err(err)?;
        z.write_all(&json).map_err(err)?;
        for m in doc.media.values() {
            // Pictures are compressed already.
            let opts = if m.mime.starts_with("image/") && m.mime != "image/svg+xml" && m.mime != "image/bmp" { stored } else { deflated };
            z.start_file(m.zip_path(), opts).map_err(err)?;
            z.write_all(&m.bytes).map_err(err)?;
        }
        z.finish().map_err(err)?;
    }
    Ok(buf.into_inner())
}

/// Reads a file's bytes.
pub fn from_bytes(bytes: &[u8]) -> Result<Document> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| Error(format!("This isn't a folio file (not a zip: {e}).")))?;
    let mut json = vec![];
    z.by_name("document.json").map_err(|_| Error("This isn't a folio file (no document.json inside).".into()))?.read_to_end(&mut json).map_err(err)?;
    let mut doc: Document = serde_json::from_slice(&json).map_err(|e| Error(format!("The folio file is damaged: {e}")))?;
    if doc.format > crate::FORMAT {
        return Err(Error(format!("This file was written by a newer folio (format {}). Update folio to open it.", doc.format)));
    }
    let ids: Vec<_> = doc.media.keys().cloned().collect();
    for id in ids {
        let path = doc.media[&id].zip_path();
        let mut data = vec![];
        if let Ok(mut f) = z.by_name(&path) {
            f.read_to_end(&mut data).map_err(err)?;
        }
        if let Some(m) = doc.media.get_mut(&id) {
            m.bytes = Arc::new(data);
        }
    }
    Ok(doc)
}

/// Writes atomically (a temporary file beside it, then a rename).
pub fn save(doc: &Document, path: &Path) -> Result<()> {
    let bytes = to_bytes(doc)?;
    write_atomic(path, &bytes)
}

pub fn load(path: &Path) -> Result<Document> {
    let bytes = std::fs::read(path).map_err(|e| Error(format!("Couldn't read {}: {e}", path.display())))?;
    from_bytes(&bytes)
}

/// Writes bytes to a file atomically.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| Error(format!("Couldn't create {}: {e}", dir.display())))?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let r = std::fs::File::create(&tmp).and_then(|mut f| {
        f.write_all(bytes)?;
        f.sync_all()
    });
    if let Err(e) = r.and_then(|()| std::fs::rename(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error(format!("Couldn't write {}: {e}", path.display())));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PageKind;

    #[test]
    fn round_trip_with_media() {
        let mut d = Document::new("Report");
        let s = d.add_page(PageKind::Sheet, Some("Data"), None).unwrap();
        d.page_mut(s).sheet_mut().unwrap().set_input(folio_calc::Addr::new(0, 0), "=1+1");
        d.add_page(PageKind::Deck, None, None).unwrap();
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x02\0\0\0\x03rest".to_vec();
        let id = d.add_media("dot.png", png.clone());
        let bytes = to_bytes(&d).unwrap();
        let back = from_bytes(&bytes).unwrap();
        assert_eq!(back.pages.len(), 3);
        assert_eq!(back.media[&id].bytes.as_slice(), png.as_slice());
        assert_eq!((back.media[&id].width, back.media[&id].height), (2, 3));
        assert_eq!(back.pages[1].sheet().unwrap().input(folio_calc::Addr::new(0, 0)), "=1+1");
        // The mimetype comes first and is stored.
        let z = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
        assert_eq!(z.name_for_index(0), Some("mimetype"));
    }

    #[test]
    fn save_and_load_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.folio");
        save(&Document::new("x"), &path).unwrap();
        assert_eq!(load(&path).unwrap().title, "x");
        assert!(from_bytes(b"not a zip").is_err());
    }
}
