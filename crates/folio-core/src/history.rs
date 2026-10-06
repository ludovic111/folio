//! One undo history for the whole file, shared by every client (window, agent, CLI, MCP).
//!
//! A step keeps the document as it was before the change. Documents are cheap to copy (pages
//! behind `Arc`, persistent collections inside), so a snapshot costs little and undo is exact
//! whatever the change was. Edits sharing a coalesce key within about a second fold into one
//! step (typing a word, dragging a shape); a batch makes many edits one step.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::Document;

/// How far apart two edits with the same key can be and still fold into one step.
pub const COALESCE_WINDOW: Duration = Duration::from_millis(1200);
/// Steps kept (the oldest go first).
pub const MAX_STEPS: usize = 400;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepInfo {
    pub seq: u64,
    pub label: String,
    /// `window`, `agent`, `cli` or `mcp`.
    pub source: String,
    pub at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone)]
struct Step {
    info: StepInfo,
    before: Document,
}

struct Batch {
    before: Document,
    label: String,
    source: String,
    depth: usize,
}

#[derive(Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    seq: u64,
    last: Option<(String, String, Instant)>,
    batch: Option<Batch>,
}

impl History {
    /// Records that `before` became the current document through an edit. Returns false when
    /// the edit folded into the previous step.
    pub fn record(&mut self, label: &str, source: &str, coalesce: Option<&str>, before: Document) -> bool {
        if self.batch.is_some() {
            return false;
        }
        self.redo.clear();
        let now = Instant::now();
        if let (Some(key), Some((k, s, t))) = (coalesce, &self.last) {
            let gesture = key.starts_with("gesture:");
            if k == key && s == source && (gesture || now.duration_since(*t) < COALESCE_WINDOW) && !self.undo.is_empty() {
                self.last = Some((key.to_string(), source.to_string(), now));
                return false;
            }
        }
        self.last = coalesce.map(|k| (k.to_string(), source.to_string(), now));
        self.push(label, source, before);
        true
    }

    fn push(&mut self, label: &str, source: &str, before: Document) {
        self.seq += 1;
        self.undo.push(Step { info: StepInfo { seq: self.seq, label: label.to_string(), source: source.to_string(), at: chrono::Utc::now() }, before });
        if self.undo.len() > MAX_STEPS {
            self.undo.remove(0);
        }
    }

    /// Ends coalescing (an undo, a different edit, or a click elsewhere).
    pub fn break_coalesce(&mut self) {
        self.last = None;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() && self.batch.is_none()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty() && self.batch.is_none()
    }

    /// Takes the last step back: returns the document to restore and the step's info.
    pub fn undo(&mut self, current: Document) -> Option<(Document, StepInfo)> {
        if self.batch.is_some() {
            return None;
        }
        let step = self.undo.pop()?;
        self.last = None;
        self.redo.push(Step { info: step.info.clone(), before: current });
        Some((step.before, step.info))
    }

    pub fn redo(&mut self, current: Document) -> Option<(Document, StepInfo)> {
        if self.batch.is_some() {
            return None;
        }
        let step = self.redo.pop()?;
        self.last = None;
        self.undo.push(Step { info: step.info.clone(), before: current });
        Some((step.before, step.info))
    }

    pub fn begin_batch(&mut self, label: &str, source: &str, current: &Document) {
        match &mut self.batch {
            Some(b) => b.depth += 1,
            None => self.batch = Some(Batch { before: current.clone(), label: label.to_string(), source: source.to_string(), depth: 1 }),
        }
    }

    pub fn in_batch(&self) -> bool {
        self.batch.is_some()
    }

    /// Closes the batch: one step for everything in it (none when nothing changed).
    pub fn end_batch(&mut self, current: &Document) {
        let Some(b) = &mut self.batch else { return };
        if b.depth > 1 {
            b.depth -= 1;
            return;
        }
        let b = self.batch.take().unwrap();
        if !b.before.same_as(current) {
            self.redo.clear();
            self.last = None;
            self.push(&b.label, &b.source, b.before);
        }
    }

    /// Abandons the batch: returns the document as it was when it began.
    pub fn rollback_batch(&mut self) -> Option<Document> {
        self.batch.take().map(|b| b.before)
    }

    /// The position in the history now (the last step's number, 0 for none).
    pub fn checkpoint(&self) -> u64 {
        self.undo.last().map(|s| s.info.seq).unwrap_or(0)
    }

    /// How many steps were made after `checkpoint` (what `revert_to` would undo).
    pub fn steps_since(&self, checkpoint: u64) -> usize {
        self.undo.iter().rev().take_while(|s| s.info.seq > checkpoint).count()
    }

    pub fn undo_list(&self) -> Vec<StepInfo> {
        self.undo.iter().rev().map(|s| s.info.clone()).collect()
    }

    pub fn redo_list(&self) -> Vec<StepInfo> {
        self.redo.iter().rev().map(|s| s.info.clone()).collect()
    }

    pub fn clear(&mut self) {
        *self = History { seq: self.seq, ..Default::default() };
    }
}
