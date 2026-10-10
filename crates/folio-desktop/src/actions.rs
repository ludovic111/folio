//! Keyboard shortcuts and the menu bar.
//!
//! [`SHORTCUTS`] is the one list of keys: it binds them, fills the shortcuts sheet and the
//! palette, and gives tooltips their hint in this platform's notation (`⌘B` on macOS, `Ctrl+B`
//! elsewhere). Keys a person coming from Word, Excel, PowerPoint or Google's apps expects are
//! where they expect them.

use gpui::{Action, App, KeyBinding, Menu, MenuItem, OsAction, SharedString, actions};

actions!(
    folio,
    [
        Quit,
        About,
        NewFile,
        OpenFile,
        CloseFile,
        Save,
        SaveAs,
        Export,
        Print,
        Undo,
        Redo,
        Palette,
        ShowShortcuts,
        OpenSettings,
        ToggleAgent,
        ToggleInspector,
        ToggleSidebar,
        ToggleTheme,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        Present,
        NewDocPage,
        NewSheetPage,
        NewDeckPage,
        NextPage,
        PrevPage,
        Find,
        OpenPlugins,
        // Text (documents and text boxes).
        Bold,
        Italic,
        Underline,
        Strike,
        InsertLink,
        Heading1,
        Heading2,
        Heading3,
        NormalText,
        BulletList,
        NumberList,
        CheckList,
        AlignLeft,
        AlignCenter,
        AlignRight,
        AlignJustify,
        AddComment,
        PageBreak,
        // Moving and editing the caret.
        Backspace,
        DeleteForward,
        DeleteWordBack,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        LineStart,
        LineEnd,
        SelectLineStart,
        SelectLineEnd,
        DocStart,
        DocEnd,
        SelectAll,
        Copy,
        Cut,
        Paste,
        Enter,
        ShiftEnter,
        Tab,
        ShiftTab,
        Escape,
        PageUp,
        PageDown,
        // Sheets.
        EditCell,
        FillDown,
        FillRight,
        JumpUp,
        JumpDown,
        JumpLeft,
        JumpRight,
        AutoSum,
        // Decks.
        Duplicate,
        NewSlide,
        NudgeLeft,
        NudgeRight,
        NudgeUp,
        NudgeDown,
    ]
);

/// Where a shortcut works.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    /// The whole window.
    App,
    /// Editing text: documents and text boxes on slides.
    Text,
    /// The grid of a sheet (and its cell editor).
    Sheet,
    /// A slide with shapes selected.
    Deck,
}

pub struct Shortcut {
    /// `M-` is ⌘ on macOS and Ctrl elsewhere.
    pub keys: &'static [&'static str],
    pub action: fn() -> Box<dyn Action>,
    pub scope: Scope,
    pub label: &'static str,
    /// The shortcuts sheet's sections.
    pub group: &'static str,
}

macro_rules! sc {
    ($keys:expr, $a:expr, $scope:ident, $label:expr, $group:expr) => {
        Shortcut { keys: &$keys, action: || Box::new($a), scope: Scope::$scope, label: $label, group: $group }
    };
}

pub const SHORTCUTS: &[Shortcut] = &[
    sc!(["M-n"], NewFile, App, "New file", "File"),
    sc!(["M-o"], OpenFile, App, "Open or import…", "File"),
    sc!(["M-s"], Save, App, "Save", "File"),
    sc!(["M-shift-s"], SaveAs, App, "Save as…", "File"),
    sc!(["M-alt-e"], Export, App, "Export (PDF, Word, Excel, PowerPoint…)", "File"),
    sc!(["M-p"], Print, App, "Export as PDF to print", "File"),
    sc!(["M-w"], CloseFile, App, "Close the file", "File"),
    sc!(["M-z"], Undo, App, "Undo", "Edit"),
    sc!(["M-shift-z", "M-y"], Redo, App, "Redo", "Edit"),
    sc!(["M-f"], Find, App, "Find and replace", "Edit"),
    sc!(["M-shift-p"], Palette, App, "Command palette", "View"),
    sc!(["M-/"], ShowShortcuts, App, "Keyboard shortcuts", "View"),
    sc!(["M-,"], OpenSettings, App, "Settings", "View"),
    sc!(["M-j"], ToggleAgent, App, "Agent panel", "View"),
    sc!(["M-alt-i"], ToggleInspector, App, "Inspector", "View"),
    sc!(["M-\\"], ToggleSidebar, App, "Pages sidebar", "View"),
    sc!(["M-="], ZoomIn, App, "Zoom in", "View"),
    sc!(["M--"], ZoomOut, App, "Zoom out", "View"),
    sc!(["M-0"], ZoomReset, App, "Actual size", "View"),
    sc!(["M-enter", "f5"], Present, App, "Present the deck", "View"),
    sc!(["M-alt-]", "ctrl-pagedown"], NextPage, App, "Next page", "Pages"),
    sc!(["M-alt-[", "ctrl-pageup"], PrevPage, App, "Previous page", "Pages"),
    sc!(["M-b"], Bold, Text, "Bold", "Text"),
    sc!(["M-i"], Italic, Text, "Italic", "Text"),
    sc!(["M-u"], Underline, Text, "Underline", "Text"),
    sc!(["M-shift-x"], Strike, Text, "Strikethrough", "Text"),
    sc!(["M-k"], InsertLink, Text, "Link", "Text"),
    sc!(["M-alt-1"], Heading1, Text, "Heading 1", "Text"),
    sc!(["M-alt-2"], Heading2, Text, "Heading 2", "Text"),
    sc!(["M-alt-3"], Heading3, Text, "Heading 3", "Text"),
    sc!(["M-alt-0"], NormalText, Text, "Normal text", "Text"),
    sc!(["M-shift-8"], BulletList, Text, "Bulleted list", "Text"),
    sc!(["M-shift-7"], NumberList, Text, "Numbered list", "Text"),
    sc!(["M-shift-9"], CheckList, Text, "Checklist", "Text"),
    sc!(["M-shift-l"], AlignLeft, Text, "Align left", "Text"),
    sc!(["M-shift-e"], AlignCenter, Text, "Centre", "Text"),
    sc!(["M-shift-r"], AlignRight, Text, "Align right", "Text"),
    sc!(["M-shift-j"], AlignJustify, Text, "Justify", "Text"),
    sc!(["M-alt-m"], AddComment, Text, "Comment", "Text"),
    sc!(["M-shift-enter"], PageBreak, Text, "Page break", "Text"),
    sc!(["M-d"], FillDown, Sheet, "Fill down", "Sheets"),
    sc!(["M-r"], FillRight, Sheet, "Fill right", "Sheets"),
    sc!(["f2"], EditCell, Sheet, "Edit the cell", "Sheets"),
    sc!(["alt-="], AutoSum, Sheet, "AutoSum", "Sheets"),
    sc!(["M-up"], JumpUp, Sheet, "Jump to the edge up", "Sheets"),
    sc!(["M-down"], JumpDown, Sheet, "Jump to the edge down", "Sheets"),
    sc!(["M-left"], JumpLeft, Sheet, "Jump to the edge left", "Sheets"),
    sc!(["M-right"], JumpRight, Sheet, "Jump to the edge right", "Sheets"),
    sc!(["M-d"], Duplicate, Deck, "Duplicate", "Slides"),
    sc!(["M-shift-m"], NewSlide, Deck, "New slide", "Slides"),
];

/// Keys the editors handle themselves (moving the caret, deleting…): bound in their contexts,
/// not listed in the sheet.
fn editing_bindings() -> Vec<KeyBinding> {
    let m = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    let word = if cfg!(target_os = "macos") { "alt" } else { "ctrl" };
    let mut v = vec![];
    for ctx in ["DocEditor", "SheetEditor", "DeckEditor"] {
        let c = Some(ctx);
        v.extend([
            KeyBinding::new("backspace", Backspace, c),
            KeyBinding::new("delete", DeleteForward, c),
            KeyBinding::new(&format!("{word}-backspace"), DeleteWordBack, c),
            KeyBinding::new("left", Left, c),
            KeyBinding::new("right", Right, c),
            KeyBinding::new("up", Up, c),
            KeyBinding::new("down", Down, c),
            KeyBinding::new("shift-left", SelectLeft, c),
            KeyBinding::new("shift-right", SelectRight, c),
            KeyBinding::new("shift-up", SelectUp, c),
            KeyBinding::new("shift-down", SelectDown, c),
            KeyBinding::new(&format!("{word}-left"), WordLeft, c),
            KeyBinding::new(&format!("{word}-right"), WordRight, c),
            KeyBinding::new(&format!("{word}-shift-left"), SelectWordLeft, c),
            KeyBinding::new(&format!("{word}-shift-right"), SelectWordRight, c),
            KeyBinding::new("home", LineStart, c),
            KeyBinding::new("end", LineEnd, c),
            KeyBinding::new("shift-home", SelectLineStart, c),
            KeyBinding::new("shift-end", SelectLineEnd, c),
            KeyBinding::new(&format!("{m}-a"), SelectAll, c),
            KeyBinding::new(&format!("{m}-c"), Copy, c),
            KeyBinding::new(&format!("{m}-x"), Cut, c),
            KeyBinding::new(&format!("{m}-v"), Paste, c),
            KeyBinding::new("enter", Enter, c),
            KeyBinding::new("shift-enter", ShiftEnter, c),
            KeyBinding::new("tab", Tab, c),
            KeyBinding::new("shift-tab", ShiftTab, c),
            KeyBinding::new("escape", Escape, c),
            KeyBinding::new("pageup", PageUp, c),
            KeyBinding::new("pagedown", PageDown, c),
        ]);
        if cfg!(target_os = "macos") {
            v.extend([
                KeyBinding::new("cmd-left", LineStart, c),
                KeyBinding::new("cmd-right", LineEnd, c),
                KeyBinding::new("cmd-shift-left", SelectLineStart, c),
                KeyBinding::new("cmd-shift-right", SelectLineEnd, c),
                KeyBinding::new("cmd-up", DocStart, c),
                KeyBinding::new("cmd-down", DocEnd, c),
            ]);
        } else {
            v.extend([KeyBinding::new("ctrl-home", DocStart, c), KeyBinding::new("ctrl-end", DocEnd, c)]);
        }
    }
    // Slides: arrows nudge the selected shapes (the deck view decides when they move the caret).
    v
}

/// `M-` → `cmd-` on macOS, `ctrl-` elsewhere.
fn concrete(keys: &str) -> String {
    keys.replace("M-", if cfg!(target_os = "macos") { "cmd-" } else { "ctrl-" })
}

pub fn bind(cx: &mut App) {
    let mut b: Vec<KeyBinding> = vec![];
    for s in SHORTCUTS {
        let context = match s.scope {
            Scope::App => if s.keys.iter().any(|k| k.starts_with("M-") || k.starts_with("f")) { "Workspace" } else { "Workspace && !TextInput" },
            Scope::Text => "DocEditor || DeckText",
            Scope::Sheet => "SheetEditor",
            Scope::Deck => "DeckEditor && !DeckText",
        };
        for k in s.keys {
            match KeyBinding::load(&concrete(k), (s.action)(), Some(gpui::KeyBindingContextPredicate::parse(context).expect("valid context").into()), false, None, &gpui::DummyKeyboardMapper) {
                Ok(binding) => b.push(binding),
                Err(e) => tracing::warn!("the key {k} of {} doesn't parse: {e}", s.label),
            }
        }
    }
    b.extend(editing_bindings());
    b.extend(crate::ui::input::bindings());
    b.push(KeyBinding::new(if cfg!(target_os = "macos") { "cmd-q" } else { "ctrl-q" }, Quit, None));
    b.push(KeyBinding::new("escape", Escape, Some("Workspace")));
    for (k, next) in [("right", true), ("down", true), ("space", true), ("pagedown", true), ("enter", true), ("left", false), ("up", false), ("pageup", false), ("backspace", false)] {
        if next {
            b.push(KeyBinding::new(k, Right, Some("Presenting")));
        } else {
            b.push(KeyBinding::new(k, Left, Some("Presenting")));
        }
    }
    cx.bind_keys(b);
}

/// The shortcut of an action, as this platform writes it.
pub fn hint(action: &dyn Action) -> Option<SharedString> {
    SHORTCUTS.iter().find(|s| (s.action)().name() == action.name()).and_then(|s| s.keys.first()).map(|k| keys_label(k))
}

/// `"Undo"` → `"Undo (⌘Z)"`.
pub fn tip(label: &str, action: &dyn Action) -> SharedString {
    match hint(action) {
        Some(h) => format!("{label} ({h})").into(),
        None => label.to_string().into(),
    }
}

/// A table keystroke (`"M-shift-z"`) in this platform's notation.
pub fn keys_label(keys: &str) -> SharedString {
    keys_label_for(keys, cfg!(target_os = "macos")).into()
}

pub fn keys_label_for(keys: &str, mac: bool) -> String {
    let parts: Vec<&str> = keys.split('-').collect();
    let (mods, key) = parts.split_at(parts.len().saturating_sub(1));
    let key = match key.first().copied().unwrap_or("") {
        "" => "-".to_string(),
        "enter" => if mac { "↩".into() } else { "Enter".into() },
        "backspace" => if mac { "⌫".into() } else { "Backspace".into() },
        "up" => "↑".into(),
        "down" => "↓".into(),
        "left" => "←".into(),
        "right" => "→".into(),
        "pageup" => "PgUp".into(),
        "pagedown" => "PgDn".into(),
        "escape" => "Esc".into(),
        k if k.len() == 1 => k.to_uppercase(),
        k => k[..1].to_uppercase() + &k[1..],
    };
    let mut out = String::new();
    let mut names = vec![];
    for m in mods {
        let (mac_sym, other) = match *m {
            "M" => ("⌘", "Ctrl"),
            "cmd" => ("⌘", "Ctrl"),
            "ctrl" => ("⌃", "Ctrl"),
            "alt" => ("⌥", "Alt"),
            "shift" => ("⇧", "Shift"),
            _ => ("", ""),
        };
        if mac {
            out.push_str(mac_sym);
        } else {
            names.push(other);
        }
    }
    if mac {
        // macOS order: ⌃⌥⇧⌘.
        let order = ['⌃', '⌥', '⇧', '⌘'];
        let mut syms: Vec<char> = out.chars().collect();
        syms.sort_by_key(|c| order.iter().position(|o| o == c).unwrap_or(9));
        syms.into_iter().collect::<String>() + &key
    } else {
        names.push(&key);
        names.join("+")
    }
}

pub fn menus() -> Vec<Menu> {
    vec![
        Menu::new("folio").items([MenuItem::action("About folio", About), MenuItem::separator(), MenuItem::action("Settings…", OpenSettings), MenuItem::action("Plugins…", OpenPlugins), MenuItem::separator(), MenuItem::action("Quit folio", Quit)]),
        Menu::new("File").items([
            MenuItem::action("New", NewFile),
            MenuItem::action("Open or Import…", OpenFile),
            MenuItem::separator(),
            MenuItem::action("Save", Save),
            MenuItem::action("Save As…", SaveAs),
            MenuItem::action("Export…", Export),
            MenuItem::separator(),
            MenuItem::action("New Document Page", NewDocPage),
            MenuItem::action("New Sheet Page", NewSheetPage),
            MenuItem::action("New Deck Page", NewDeckPage),
            MenuItem::separator(),
            MenuItem::action("Close", CloseFile),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", Undo, OsAction::Undo),
            MenuItem::os_action("Redo", Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", Cut, OsAction::Cut),
            MenuItem::os_action("Copy", Copy, OsAction::Copy),
            MenuItem::os_action("Paste", Paste, OsAction::Paste),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
            MenuItem::separator(),
            MenuItem::action("Find and Replace", Find),
        ]),
        Menu::new("Format").items([
            MenuItem::action("Bold", Bold),
            MenuItem::action("Italic", Italic),
            MenuItem::action("Underline", Underline),
            MenuItem::action("Strikethrough", Strike),
            MenuItem::separator(),
            MenuItem::action("Heading 1", Heading1),
            MenuItem::action("Heading 2", Heading2),
            MenuItem::action("Heading 3", Heading3),
            MenuItem::action("Normal Text", NormalText),
            MenuItem::separator(),
            MenuItem::action("Bulleted List", BulletList),
            MenuItem::action("Numbered List", NumberList),
            MenuItem::action("Checklist", CheckList),
        ]),
        Menu::new("View").items([
            MenuItem::action("Pages", ToggleSidebar),
            MenuItem::action("Inspector", ToggleInspector),
            MenuItem::action("Agent", ToggleAgent),
            MenuItem::separator(),
            MenuItem::action("Zoom In", ZoomIn),
            MenuItem::action("Zoom Out", ZoomOut),
            MenuItem::action("Actual Size", ZoomReset),
            MenuItem::separator(),
            MenuItem::action("Present", Present),
            MenuItem::action("Dark or Light", ToggleTheme),
            MenuItem::action("Command Palette", Palette),
        ]),
        Menu::new("Help").items([MenuItem::action("Keyboard Shortcuts", ShowShortcuts)]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_follow_the_platform() {
        assert_eq!(keys_label_for("M-shift-z", true), "⇧⌘Z");
        assert_eq!(keys_label_for("M-shift-z", false), "Ctrl+Shift+Z");
        assert_eq!(keys_label_for("M-alt-1", true), "⌥⌘1");
        assert_eq!(keys_label_for("f5", false), "F5");
    }

    #[test]
    fn every_shortcut_parses() {
        for s in SHORTCUTS {
            for k in s.keys {
                assert!(gpui::Keystroke::parse(&concrete(k).split('-').collect::<Vec<_>>().join("-")).is_ok() || k.contains("--") || k.ends_with('-'), "{k}");
            }
        }
    }
}
