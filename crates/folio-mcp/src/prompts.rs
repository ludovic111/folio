//! MCP prompts: ready starts for common jobs, written against the registry's tools.

use serde_json::Value;

pub struct Prompt {
    pub name: &'static str,
    pub description: &'static str,
    /// name, description, required
    pub arguments: &'static [(&'static str, &'static str, bool)],
    pub render: fn(&Value) -> String,
}

/// A string argument, or `default` when it is missing or blank.
pub fn arg<'a>(arguments: &'a Value, key: &str, default: &'a str) -> &'a str {
    arguments.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).unwrap_or(default)
}

pub const PROMPTS: [Prompt; 5] = [
    Prompt {
        name: "write-document",
        description: "Write a document page from a brief: structure, headings, lists and a table where it helps.",
        arguments: &[("brief", "What the document is about and who reads it", true), ("length", "About how long (default: one or two pages)", false)],
        render: |a| {
            format!(
                "Write a document: {brief}. Aim for {length}. Start with file_overview; if no file is open, file_new kind=doc. \
                 Write it with doc_write in Markdown: a # title, ## sections, short paragraphs, lists where there are lists, a table where numbers compare. \
                 Then read it back with doc_read markdown=true, tighten what reads badly with doc_replace or doc_setText, and set the page up (doc_setup: size, margins, a footer with {{page}} / {{pages}}).",
                brief = arg(a, "brief", ""),
                length = arg(a, "length", "one or two pages"),
            )
        },
    },
    Prompt {
        name: "build-sheet",
        description: "Build a working spreadsheet: data, formulas, formats, a chart.",
        arguments: &[("purpose", "What the sheet is for (a budget, a tracker, a forecast…)", true), ("data", "Data to start from, if any", false)],
        render: |a| {
            format!(
                "Build a sheet for {purpose}. {data} Add a sheet page (page_add kind=sheet name=…) unless the file has one for this. \
                 Lay it out as a table: a header row, one row per item, totals with formulas (=SUM, =AVERAGE, =IF, =XLOOKUP…; sheet_functions lists them). \
                 Write it with sheet_setRange in as few calls as you can; the answer lists cells showing errors: fix them. \
                 Format with sheet_format (bold header, number formats like #,##0.00, 0%, yyyy-mm-dd), freeze the header (sheet_freeze rows=1), fit columns (sheet_resize fit=A:F), \
                 and add a chart of what matters (sheet_addChart). Finish with sheet_read and say what each formula does.",
                purpose = arg(a, "purpose", ""),
                data = match arg(a, "data", "") {
                    "" => "Invent plausible example rows only if the person asked for examples; otherwise leave clear placeholders.".to_string(),
                    d => format!("Start from this data: {d}."),
                },
            )
        },
    },
    Prompt {
        name: "deck-from-file",
        description: "Make slides from what the file already holds (a document's sections, a sheet's numbers).",
        arguments: &[("audience", "Who will watch it", false), ("slides", "About how many slides (default 6–10)", false)],
        render: |a| {
            format!(
                "Make a deck for {audience} of about {slides} slides from this file. Read file_overview, then doc_read the documents and sheet_read the sheets. \
                 Add a deck page (page_add kind=deck), then one deck_addSlide per idea: a title slide, one slide per section with three to five short bullets (body lines), \
                 and for numbers a chart that reads the sheet live (deck_addChart source='Sheet'!A1:C9) or a live table (deck_addTable link=…). Put what to say in each slide's notes. \
                 Pick a theme with deck_setTheme. Finish with deck_read and list the slides.",
                audience = arg(a, "audience", "the team"),
                slides = arg(a, "slides", "6–10"),
            )
        },
    },
    Prompt {
        name: "review-document",
        description: "Review a document: comments on what to fix, tracked changes for wording.",
        arguments: &[("focus", "What to look for (clarity, tone, facts, length…)", false)],
        render: |a| {
            format!(
                "Review the document page shown (file_overview says which), looking for {focus}. Read it with doc_read. \
                 Leave a comment (doc_comment find=… text=…) on each place that needs the author's decision. For wording you are sure of, turn tracked changes on (doc_trackChanges on=true) \
                 and edit with doc_replace, so the author can accept or reject each change. Finish with a short summary of what you found.",
                focus = arg(a, "focus", "clarity and mistakes"),
            )
        },
    },
    Prompt {
        name: "spreadsheet-function-plugin",
        description: "Write a custom spreadsheet function in Rust as a folio plugin (=MYFUNC()).",
        arguments: &[("function", "What the function computes and its name", true)],
        render: |a| {
            format!(
                "Make a folio plugin that adds this spreadsheet function: {f}. Follow plugin_guide exactly: plugin_toolchain (Rust must be installed), plugin_new kind=functions, \
                 write src/lib.rs with plugin_writeSource, plugin_build until it is green (fix from the structured errors), then plugin_publishLocal. \
                 Try it: sheet_evaluate formula=\"=YOURFUNC(…)\" with a few inputs, including edge cases (empty cells, text, errors). Needs the plugins permission.",
                f = arg(a, "function", ""),
            )
        },
    },
];
