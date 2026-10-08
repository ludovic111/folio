# folio

lsuite's office app: documents, sheets and slides in one `.folio` file, with live links between
them. A native Rust app: the window is **GPUI** (pinned to Zed commit `7733b99…`, exactly like
kimchi, `runtime_shaders` on macOS), and every action goes through one command registry. See
README.md and docs/.

The name is a working name chosen on 2026-10-07 for the owner: renaming means the crate names
(`folio-*`), the binary names (`folio`, `folio-cli`, `folio-mcp`), `FOLIO_*` environment variables,
the `.folio` extension and MIME type (`application/vnd.lsuite.folio`), the bundle id
`xyz.lsuite.folio`, `~/.lsuite/apps/folio.json`, the discovery `app` field and the strings in the
window. A search for `folio` finds them all.

```
crates/folio-calc     formula engine: parser, evaluator, ~150 functions, number formats, dates (Excel's 1900
                      system), dependency graph, formula rewriting (fill, insert rows, rename sheets)
crates/folio-core     the model (text.rs rich text + flow editing, sheet.rs, deck.rs, chart.rs, links.rs),
                      Document with pages behind Arc and imbl collections, History (snapshot undo, coalescing,
                      batches, checkpoints), Editor, recalc.rs (keeps calc in step by diffing cells), file.rs (.folio)
crates/folio-layout   cosmic-text layout with the bundled faces: paragraphs, pagination, slide text, chart geometry,
                      PNG rasters. The window paints its glyphs, the PDF writes them: screen = print
crates/folio-io       formats: docx, xlsx (calamine / rust_xlsxwriter), pptx, odt/ods/odp, csv, markdown, html, pdf (krilla)
crates/folio-control  registry (commands/mod.rs lists every spec), session (autosave), permissions, bridge,
                      discovery, account (lsuite AI), plugins host, templates, overview
crates/folio-agent    the built-in agent (lsuite AI, Claude Code, Codex, Anthropic, OpenAI-compatible, Ollama)
crates/folio-plugin   the plugin SDK (frozen repr(C) ABI, spreadsheet functions)
crates/folio-desktop  the window (package/binary `folio`): store.rs, app.rs, views/, ui/, theme.rs, paint.rs
crates/folio-cli      `folio-cli`;  crates/folio-mcp: `folio-mcp`
```

Rules that keep it working:

- **A feature is a command first.** Add the spec in `folio-control/src/commands/mod.rs`, the handler in its
  family file, then call it from the window with `Store::run` (background) or `Store::run_now` (quick
  edits that must show in the same frame: typing, a cell, a drag). Never change the document from the
  window directly. Regenerate `docs/COMMANDS.md` with `cargo run -p folio-cli -- docs` (a test fails
  otherwise). Window-only commands (`ui.*`, `deck.present`, `app.quit`) are carried out in `app.rs`.
- **One undo history.** `Session::edit` wraps `Editor::edit`: one step per command, or folded into the last
  one with a `coalesce` key (typing uses `"typing"`, drags `gesture:<id>`). `file.batch` is one step and
  rolls back on failure. Undo restores a snapshot of the whole document (cheap: pages are `Arc`, blocks and
  cells are `imbl` collections), then `recalc` brings formulas back in step.
- **Values follow inputs.** Cells keep `input` (what was typed) and `value` (computed). Don't set `value` by
  hand: `Calc::sync` diffs every sheet against what it saw and recomputes dependents.
- **Screen = print.** Document pages are laid out by folio-layout in points; the window scales by
  `zoom × 4/3` and paints the same glyph ids (`paint.rs`). A change in layout shows in the window, the PDF
  and the page count at once.
- **Files save themselves** (`Session::schedule_save`, 0.7 s after a change; at once in headless sessions).
  New files live in `<data>/untitled/` until Save as; opened .docx/.xlsx/.pptx are copies (the original is
  never overwritten; `file.export` writes back).
- **Docs follow the code**: `docs/FILE_FORMAT.md` for the model, `docs/AI_CONTROL.md`, generated
  `docs/COMMANDS.md`, CHANGELOG.md for each release.
- GPUI API: grep the pinned source in `~/.cargo/git/checkouts/zed-*/7733b99/crates/gpui`. Views keep retained
  state in their entity; text fields are `ui::input::TextInput`; icons are lucide (`ui::icon`).
- Shortcuts: one table, `actions::SHORTCUTS` (binds keys, fills the sheet and the palette). Editors bind the
  caret keys in their contexts (`DocEditor`, `SheetEditor`, `DeckEditor`, `DeckText`).
- Look: lsuite design system v2, the same as kimchi (`theme.rs`, `ui/grain.rs` copied from kimchi): black and
  white, square, grain behind the chrome, solid work (paper is white in both modes), Chakra Petch + IBM Plex
  Mono for the interface, IBM Plex Sans/Serif/Mono for documents. Red only for what destroys. Logos of other
  apps keep their colours (`assets/logos/SOURCES.md`). Every area titled, tools boxed by kind.
- Testing the app on Linux: `vscreen start target/debug/folio` (VSCREEN=folio-night), `vscreen shot`;
  `FOLIO_DATA_DIR`, `FOLIO_CONFIG_DIR`, `LSUITE_HOME` to scratch folders, `FOLIO_NO_SETUP=1` skips the
  first-run setup, `FOLIO_WINDOW_SIZE=1600x1000`; drive it with `target/debug/folio-cli`.

## lsuite (notes 2026-10-07)

folio is part of **lsuite** with ryolune (music), kimchi (video), zenith (code) and nori (images); its page
is lsuite.xyz/folio. Contract: `../lsuite/STANDARD.md`, `PLUGINS.md`, `AI.md`, `design/DESIGN.md`.

- **Linux only while lsuite is in beta** (owner, 2026-10-08): macOS and Windows are "coming soon". Their
  code and scripts stay in the source, but CI (`ci.yml`), `release.yml` and `publish-build.sh` build and
  ship Linux only (latest.json lists only Linux platforms); kimchi's `suite-build.yml` is being made
  Linux only too. README and the CHANGELOG say so.
- [x] Command registry, one undo history, `file.overview`; CLI and MCP (`--live`, `--file`).
- [x] Discovery: `~/.lsuite/apps/folio.json`, kind `office`; hand-offs: `handoff.image` from nori and kimchi
      through their CLIs (best effort: nori's export command is looked up from its `app.commands`).
- [x] lsuite AI: `account.*`, the shared `~/.lsuite/account.json`, loopback sign-in, first in the agent's
      providers and in the first-run setup.
- [x] Design system v2, one-ink mark and icon (`scripts/gen-mark.py`).
- [x] Signed auto-update (`update.rs`), through lsuite (DISTRIBUTION.md, 0.2.0): reads
      `<server>/api/apps/folio/latest.json` with `Authorization: Bearer` from `~/.lsuite/account.json`
      (`LSUITE_HOME`; server `LSUITE_ACCOUNT_SERVER`, else the account's, else lsuite.xyz); the token goes
      only to that server (the file route's redirect drops it); signed out → `sign_in` status "Sign in to
      lsuite (in the lsuite app) to get updates"; `FOLIO_UPDATE_URL` still overrides (no token). Tested
      against a fake server (`update.rs` tests).
- [x] Releases: `release.yml` makes a draft; folio's suite builds come from kimchi's `suite-build.yml`
      (app=folio). `scripts/publish-build.sh <version> [<run-id>]` copies a run's Linux artifact (its Linux
      job must have succeeded; the others may be cancelled) or the draft's Linux files to
      `ludovic111/lsuite-builds` as `folio-v<version>` with latest.json (folio-release) and SHA256SUMS.
- [x] **Agent harness** (HARNESS.md, 0.2.0), `folio-control/src/harness/`:
  1. Brief: `brief.md` + the skills' index = `harness.brief` = the built-in agent's system prompt
     (`folio-agent` `system_prompt()`) = `folio-mcp` `instructions` (shortened for the built-in agent,
     which has it already). A test keeps it 800–1,500 words and checks every command it names exists.
  2. Skills: 12 in `harness/skills/*.md` (`# Title`, `When:`, `## Steps`, `## Checks`); `harness.skills`,
     `harness.skill`; MCP prompts `skill-<name>` and resources `folio://skills/<name>`.
  3. Live context: `harness/context.rs` (`glance` for the panel, `context` = glance + page sizes + open
     problems + the person's changes since `seq`); the API loop appends a `Part::Context` after a step's
     results when it changed; `folio-mcp` appends notes (saved path, `<context>` when it changed, the
     finish-routine reminder after an edit until `harness.check`/`harness.look`) to the text AND to
     `structuredContent.harnessNotes`: Claude Code shows `structuredContent` instead of the text when
     both exist. Results with pictures have no `structuredContent`. Test: `folio-mcp` `harness_notes_…`.
  4. Eyes: `harness.look` (`harness/look.rs`: `page_png`, `slide_png`, folio-layout's new `sheet_png`)
     → `<data>/looks/`; `vision.rs` turns `harness.look`/`ui.screenshot` paths into pictures:
     `Part::Image` per provider (kimchi's approach) and MCP image content.
  5. Checks: `harness.check` (`harness/check.rs`); finish routine in the brief and every skill.
  6. One undo per turn: the run's checkpoint + "Revert this run" (unchanged).
  7. Evals: `evals/run.py` (11 jobs in `evals/jobs.py`, `folio-cli --file … agent` headless with Claude
     Code by default), `evals/RESULTS.md`. Run before each release; a lower pass rate doesn't ship.
- [ ] Harness gaps: the 0.2.0 release run passed 11/11 with Opus (2026-10-08, `--model opus`; two deck
      jobs re-run after `slide: 1` as a number was accepted; RESULTS.md). `harness.look`
      can't show the window itself on Linux (`ui.screenshot` is macOS only); no named ranges (the brief
      says so); `default printed page` for a look is page 1, not the caret's page.

## Verified local beta (2026-10-07)

App names are always lowercase in UI and documentation. Apple silicon bundles were built on
macmini under `~/builds/lsuite-2026-10-07/` and smoke-tested through their bundled CLIs. These
are local ad-hoc-signed builds; public release, notarization and signed in-place updates remain
separate release work. Linux workspace tests and clippy passed (existing warnings remain).
Office/OpenDocument round trips preserve list kinds, page margins, sheet chart geometry,
editable slide tables and local text colours. The embedded mark is folio's generated f.
