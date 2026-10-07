//! The agent harness (lsuite's HARNESS.md): what makes an agent good at office work in folio,
//! the same for the built-in agent, an outside agent over `folio-mcp` and the CLI.
//!
//! * [`brief`]: the expert brief, one source (`brief.md` plus the skills' index) for the
//!   built-in agent's system prompt and `folio-mcp`'s `instructions`.
//! * [`skills`]: playbooks for the trade's jobs (`skills/*.md`), loaded with `harness.skill`.
//! * [`context`]: the live context an agent gets before every model step.
//! * [`look`]: a picture of a document page, a slide or a sheet range, with its numbers.
//! * [`check`]: objective checks (formula errors, holes in tables, overflowing text…).
//!
//! The `harness.*` commands (`commands/harness.rs`) serve them to every client.

pub mod check;
pub mod context;
pub mod look;

use std::sync::OnceLock;

/// The brief's hand-written part.
const BRIEF: &str = include_str!("brief.md");

/// Every skill: its name (the file's stem) and its markdown, in the order the index lists them.
const SKILL_FILES: &[(&str, &str)] = &[
    ("report-from-notes", include_str!("skills/report-from-notes.md")),
    ("letter-or-cv", include_str!("skills/letter-or-cv.md")),
    ("meeting-minutes", include_str!("skills/meeting-minutes.md")),
    ("review-document", include_str!("skills/review-document.md")),
    ("budget-model", include_str!("skills/budget-model.md")),
    ("clean-and-summarise", include_str!("skills/clean-and-summarise.md")),
    ("chart-from-data", include_str!("skills/chart-from-data.md")),
    ("fix-formula-errors", include_str!("skills/fix-formula-errors.md")),
    ("deck-from-document", include_str!("skills/deck-from-document.md")),
    ("pitch-deck", include_str!("skills/pitch-deck.md")),
    ("import-convert", include_str!("skills/import-convert.md")),
    ("function-plugin", include_str!("skills/function-plugin.md")),
];

/// One playbook.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    pub name: &'static str,
    /// Its `# Title` line.
    pub title: &'static str,
    /// Its `When:` line: when to use it.
    pub when: &'static str,
    pub markdown: &'static str,
}

fn parse(name: &'static str, markdown: &'static str) -> Skill {
    let title = markdown.lines().find_map(|l| l.strip_prefix("# ")).unwrap_or(name).trim();
    let when = markdown.lines().find_map(|l| l.strip_prefix("When:")).unwrap_or("").trim();
    Skill { name, title, when, markdown }
}

/// Every skill, in the index's order.
pub fn skills() -> Vec<Skill> {
    SKILL_FILES.iter().map(|(n, m)| parse(n, m)).collect()
}

/// One skill by name (case and `_` / spaces forgiven).
pub fn skill(name: &str) -> Result<Skill, String> {
    let key = name.trim().to_ascii_lowercase().replace(['_', ' '], "-");
    let key = key.trim_end_matches(".md");
    if let Some((n, m)) = SKILL_FILES.iter().find(|(n, _)| *n == key) {
        return Ok(parse(n, m));
    }
    let names: Vec<&str> = SKILL_FILES.iter().map(|(n, _)| *n).collect();
    let hint = crate::registry::closest(key, &names).map(|c| format!(" Did you mean {c}?")).unwrap_or_default();
    Err(format!("There is no skill \"{name}\".{hint} Skills: {}.", names.join(", ")))
}

/// The expert brief: the hand-written part, then the skills' index. Markdown, generated once.
pub fn brief() -> &'static str {
    static TEXT: OnceLock<String> = OnceLock::new();
    TEXT.get_or_init(|| {
        let mut out = BRIEF.trim_end().to_string();
        out.push_str("\n\n## Skills\n\nPlaybooks for common jobs. When one matches, load it with `harness.skill name=…` and follow its steps and checks.\n\n");
        for s in skills() {
            out.push_str(&format!("- `{}`: {}\n", s.name, s.when));
        }
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_skill_has_a_title_steps_and_checks() {
        let all = skills();
        assert!((8..=15).contains(&all.len()), "HARNESS.md asks for 8 to 15 skills");
        for s in &all {
            assert!(!s.title.is_empty() && s.title != s.name, "{}: a # Title line", s.name);
            assert!(s.when.len() > 20, "{}: a When: line", s.name);
            assert!(s.markdown.contains("## Steps") || s.markdown.contains("## Steps:"), "{}: steps", s.name);
            assert!(s.markdown.contains("## Checks"), "{}: checks", s.name);
        }
        assert_eq!(skill("Budget_Model").unwrap().name, "budget-model");
        assert!(skill("budjet-model").unwrap_err().contains("Did you mean budget-model?"));
    }

    #[test]
    fn the_brief_lists_the_skills_and_stays_in_bounds() {
        let b = brief();
        for s in skills() {
            assert!(b.contains(&format!("`{}`", s.name)));
        }
        let words = b.split_whitespace().count();
        assert!((800..=1600).contains(&words), "the brief has {words} words");
        assert!(b.contains("Finish routine"));
    }

    /// Every command a skill or the brief names exists in the registry.
    #[test]
    fn named_commands_exist() {
        let texts: Vec<&str> = std::iter::once(BRIEF).chain(SKILL_FILES.iter().map(|(_, m)| *m)).collect();
        for text in texts {
            for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.')) {
                let Some((family, verb)) = word.split_once('.') else { continue };
                if verb.is_empty() || verb.contains('.') || !verb.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
                    continue;
                }
                let families = ["file", "page", "text", "doc", "sheet", "deck", "link", "media", "history", "harness", "plugin", "app", "ui"];
                if families.contains(&family) {
                    assert!(crate::registry::spec(word).is_some(), "`{word}` is named in the harness but isn't a command");
                }
            }
        }
    }
}
