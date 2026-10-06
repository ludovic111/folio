//! The open file: the document, its undo history and its formula engine. Every change goes
//! through [`Editor::edit`], which records one undo step (or folds into the last one), keeps
//! the computed values up to date, and rolls the document back when the change fails.

use crate::history::{History, StepInfo};
use crate::recalc::Calc;
use crate::{Document, Result};

pub struct Editor {
    doc: Document,
    history: History,
    calc: Calc,
    /// Changed since the last save.
    dirty: bool,
    /// Counts every change (and undo/redo), so views know when to redraw.
    version: u64,
    step_label: String,
    step_source: String,
}

impl Editor {
    pub fn new(mut doc: Document) -> Self {
        let mut calc = Calc::new();
        calc.sync(&mut doc);
        Editor { doc, history: History::default(), calc, dirty: false, version: 0, step_label: String::new(), step_source: String::new() }
    }

    pub fn doc(&self) -> &Document {
        &self.doc
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn calc(&self) -> &Calc {
        &self.calc
    }

    pub fn calc_mut(&mut self) -> &mut Calc {
        &mut self.calc
    }

    /// Recomputes everything (after registering or removing plugin functions).
    pub fn recalc_all(&mut self) {
        self.calc.reload();
        if self.calc.sync(&mut self.doc) > 0 {
            self.version += 1;
        }
    }

    /// Changes the document. `label` names the undo step and `source` who made it (`window`,
    /// `agent`…); edits with the same `coalesce` key in a row fold into one step. When `f`
    /// fails, the document is left as it was.
    pub fn edit<R>(&mut self, label: &str, source: &str, coalesce: Option<&str>, f: impl FnOnce(&mut Document) -> Result<R>) -> Result<R> {
        let before = self.doc.clone();
        let r = match f(&mut self.doc) {
            Ok(r) => r,
            Err(e) => {
                self.doc = before;
                return Err(e);
            }
        };
        if self.doc.same_as(&before) {
            return Ok(r);
        }
        self.calc.sync(&mut self.doc);
        self.doc.meta.modified = Some(chrono::Utc::now());
        self.step_label = label.to_string();
        self.step_source = source.to_string();
        self.history.record(label, source, coalesce, before);
        self.dirty = true;
        self.version += 1;
        Ok(r)
    }

    /// Replaces the whole document as one undo step (imports into the open file, `doc.set`).
    pub fn replace(&mut self, label: &str, source: &str, doc: Document) {
        let _ = self.edit(label, source, None, |d| {
            *d = doc;
            Ok(())
        });
    }

    pub fn undo(&mut self) -> Option<StepInfo> {
        let (doc, info) = self.history.undo(self.doc.clone())?;
        self.doc = doc;
        self.after_jump();
        Some(info)
    }

    pub fn redo(&mut self) -> Option<StepInfo> {
        let (doc, info) = self.history.redo(self.doc.clone())?;
        self.doc = doc;
        self.after_jump();
        Some(info)
    }

    fn after_jump(&mut self) {
        self.calc.sync(&mut self.doc);
        self.dirty = true;
        self.version += 1;
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn break_coalesce(&mut self) {
        self.history.break_coalesce();
    }

    /// Starts a batch: every edit until [`end_batch`](Self::end_batch) is one undo step.
    pub fn begin_batch(&mut self, label: &str, source: &str) {
        self.history.begin_batch(label, source, &self.doc);
    }

    pub fn in_batch(&self) -> bool {
        self.history.in_batch()
    }

    pub fn end_batch(&mut self) {
        self.history.end_batch(&self.doc);
    }

    /// Undoes everything since the batch began and closes it.
    pub fn rollback_batch(&mut self) {
        if let Some(doc) = self.history.rollback_batch() {
            self.doc = doc;
            self.after_jump();
        }
    }

    pub fn checkpoint(&self) -> u64 {
        self.history.checkpoint()
    }

    /// Undoes every step made after `checkpoint` (an agent's run); returns how many.
    pub fn revert_to(&mut self, checkpoint: u64) -> usize {
        let n = self.history.steps_since(checkpoint);
        for _ in 0..n {
            if self.undo().is_none() {
                break;
            }
        }
        n
    }

    /// The label and source of the last change.
    pub fn last_step(&self) -> (&str, &str) {
        (&self.step_label, &self.step_source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{self, Pos};
    use crate::{PageKind, bail};
    use folio_calc::{Addr, Value};

    fn type_text(ed: &mut Editor, at: usize, s: &str) {
        ed.edit("Typing", "window", Some("typing"), |d| {
            let p = d.page_mut(0).doc_mut().unwrap();
            text::insert_text(&mut p.blocks, Pos::new(0, at), s, None);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn typing_coalesces_and_undoes() {
        let mut ed = Editor::new(Document::new("t"));
        type_text(&mut ed, 0, "a");
        type_text(&mut ed, 1, "b");
        type_text(&mut ed, 2, "c");
        assert_eq!(ed.doc().pages[0].plain(), "abc");
        assert_eq!(ed.history().undo_list().len(), 1);
        ed.undo();
        assert_eq!(ed.doc().pages[0].plain(), "");
        ed.redo();
        assert_eq!(ed.doc().pages[0].plain(), "abc");
    }

    #[test]
    fn failed_edit_changes_nothing() {
        let mut ed = Editor::new(Document::new("t"));
        let r: Result<()> = ed.edit("Bad", "agent", None, |d| {
            d.title = "changed".into();
            bail("no")
        });
        assert!(r.is_err());
        assert_eq!(ed.doc().title, "t");
        assert!(!ed.can_undo());
    }

    #[test]
    fn batch_is_one_step_and_formulas_compute() {
        let mut ed = Editor::new(Document::new("t"));
        ed.begin_batch("Build", "agent");
        let s = ed.edit("Add sheet", "agent", None, |d| d.add_page(PageKind::Sheet, Some("Data"), None)).unwrap();
        for (i, v) in ["1", "2", "=A1+A2"].iter().enumerate() {
            ed.edit("Set", "agent", None, |d| {
                d.page_mut(s).sheet_mut().unwrap().set_input(Addr::new(i as u32, 0), v);
                Ok(())
            })
            .unwrap();
        }
        ed.end_batch();
        assert_eq!(ed.doc().pages[s].sheet().unwrap().value(Addr::new(2, 0)), Value::Number(3.0));
        assert_eq!(ed.history().undo_list().len(), 1);
        let cp = 0;
        assert_eq!(ed.revert_to(cp), 1);
        assert_eq!(ed.doc().pages.len(), 1);
    }
}
