//! Keeps the formula engine in step with the document.
//!
//! After every change (and every undo or redo), [`Calc::sync`] compares each sheet with the
//! copy it saw last: unchanged pages are the same `Arc`, and changed ones are diffed cell by
//! cell (`imbl` skips the parts both copies share). Changed inputs go to the engine, which
//! recomputes what depends on them through its dependency graph, and the new values are
//! written back into the cells. Adding, removing, renaming or reordering sheets reloads the
//! engine.

use std::sync::Arc;

use folio_calc::{Addr, Engine, Value};

use crate::{Document, Id, Page, PageBody};

#[derive(Default)]
pub struct Calc {
    engine: Engine,
    /// What the engine holds: one entry per sheet, in order.
    seen: Vec<(Id, String, Arc<Page>)>,
}

impl Calc {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// The engine, to register plugin functions. Call [`reload`](Self::reload) after.
    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    /// Engine sheet index of a page.
    pub fn sheet_index(&self, page: &Id) -> Option<usize> {
        self.seen.iter().position(|(id, _, _)| id == page)
    }

    /// Forgets what it saw: the next sync loads every sheet again and recomputes everything.
    pub fn reload(&mut self) {
        self.seen.clear();
    }

    /// Brings the engine and the cells' values up to date. Returns how many values changed.
    pub fn sync(&mut self, doc: &mut Document) -> usize {
        let sheets: Vec<(usize, Id, String)> = doc.pages.iter().enumerate().filter(|(_, p)| matches!(p.body, PageBody::Sheet(_))).map(|(i, p)| (i, p.id.clone(), p.name.clone())).collect();
        let same_list = sheets.len() == self.seen.len() && sheets.iter().zip(&self.seen).all(|((_, id, name), (sid, sname, _))| id == sid && name == sname);
        // Cells whose input changed, per engine sheet.
        let mut touched: Vec<Vec<Addr>> = vec![vec![]; sheets.len()];
        if !same_list {
            let names: Vec<String> = sheets.iter().map(|(_, _, n)| n.clone()).collect();
            self.engine.set_sheets(&names);
            for (si, (pi, _, _)) in sheets.iter().enumerate() {
                self.engine.clear_sheet(si);
                if let Some(s) = doc.pages[*pi].sheet() {
                    for (a, c) in s.cells.iter() {
                        if !c.input.is_empty() {
                            self.engine.set_cell(si, *a, &c.input);
                            touched[si].push(*a);
                        }
                    }
                }
            }
            self.engine.recalc_all();
        } else {
            for (si, (pi, _, _)) in sheets.iter().enumerate() {
                let new = &doc.pages[*pi];
                let old = &self.seen[si].2;
                if Arc::ptr_eq(old, new) {
                    continue;
                }
                let (Some(os), Some(ns)) = (old.sheet(), new.sheet()) else { continue };
                for d in os.cells.diff(&ns.cells) {
                    use imbl::ordmap::DiffItem::*;
                    match d {
                        Add(a, c) => {
                            if !c.input.is_empty() {
                                self.engine.set_cell(si, *a, &c.input);
                                touched[si].push(*a);
                            }
                        }
                        Update { old: (_, oc), new: (a, nc) } => {
                            if oc.input != nc.input {
                                self.engine.set_cell(si, *a, &nc.input);
                                touched[si].push(*a);
                            }
                        }
                        Remove(a, c) => {
                            if !c.input.is_empty() {
                                self.engine.set_cell(si, *a, "");
                            }
                        }
                    }
                }
            }
        }
        let changed = self.engine.recalc();
        for (si, a) in changed {
            if let Some(list) = touched.get_mut(si) {
                list.push(a);
            }
        }
        // Write the engine's values where they differ from the cells'.
        let mut count = 0;
        for (si, addrs) in touched.into_iter().enumerate() {
            if addrs.is_empty() {
                continue;
            }
            let pi = sheets[si].0;
            let updates: Vec<(Addr, Value)> = {
                let s = doc.pages[pi].sheet().unwrap();
                addrs
                    .into_iter()
                    .filter_map(|a| {
                        let v = self.engine.value(si, a);
                        let cur = s.cells.get(&a)?;
                        (cur.value != v).then_some((a, v))
                    })
                    .collect()
            };
            if updates.is_empty() {
                continue;
            }
            count += updates.len();
            if let Some(s) = doc.page_mut(pi).sheet_mut() {
                for (a, v) in updates {
                    if let Some(c) = s.cells.get_mut(&a) {
                        c.value = v;
                    }
                }
            }
        }
        self.seen = sheets.iter().map(|(pi, id, name)| (id.clone(), name.clone(), doc.pages[*pi].clone())).collect();
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PageKind;

    fn a(s: &str) -> Addr {
        Addr::parse(s).unwrap()
    }

    #[test]
    fn values_follow_inputs_and_undo() {
        let mut d = Document::new("t");
        let s = d.add_page(PageKind::Sheet, Some("Data"), None).unwrap();
        let mut calc = Calc::new();
        {
            let sh = d.page_mut(s).sheet_mut().unwrap();
            sh.set_input(a("A1"), "2");
            sh.set_input(a("A2"), "3");
            sh.set_input(a("A3"), "=A1*A2");
        }
        calc.sync(&mut d);
        assert_eq!(d.pages[s].sheet().unwrap().value(a("A3")), Value::Number(6.0));
        let before = d.clone();
        d.page_mut(s).sheet_mut().unwrap().set_input(a("A1"), "10");
        calc.sync(&mut d);
        assert_eq!(d.pages[s].sheet().unwrap().value(a("A3")), Value::Number(30.0));
        // Undo: the old document comes back; syncing keeps it consistent.
        let mut d = before;
        calc.sync(&mut d);
        assert_eq!(d.pages[s].sheet().unwrap().value(a("A3")), Value::Number(6.0));
        d.page_mut(s).sheet_mut().unwrap().set_input(a("A2"), "4");
        calc.sync(&mut d);
        assert_eq!(d.pages[s].sheet().unwrap().value(a("A3")), Value::Number(8.0));
    }

    #[test]
    fn cross_sheet_and_rename() {
        let mut d = Document::new("t");
        let s1 = d.add_page(PageKind::Sheet, Some("Data"), None).unwrap();
        let s2 = d.add_page(PageKind::Sheet, Some("Summary"), None).unwrap();
        d.page_mut(s1).sheet_mut().unwrap().set_input(a("B2"), "21");
        d.page_mut(s2).sheet_mut().unwrap().set_input(a("A1"), "=Data!B2*2");
        let mut calc = Calc::new();
        calc.sync(&mut d);
        assert_eq!(d.pages[s2].sheet().unwrap().value(a("A1")), Value::Number(42.0));
        d.rename_page(s1, "Raw").unwrap();
        calc.sync(&mut d);
        assert_eq!(d.pages[s2].sheet().unwrap().input(a("A1")), "=Raw!B2*2");
        assert_eq!(d.pages[s2].sheet().unwrap().value(a("A1")), Value::Number(42.0));
    }
}
