<p align="center">
  <img src="brand/icon.png" width="112" alt="folio" />
</p>
<p align="center">
  <a href="https://lsuite.xyz/folio">Website</a> ·
  <a href="https://lsuite.xyz/launcher">Get it in the lsuite app</a> ·
  <a href="https://lsuite.xyz/folio/support">Support</a>
</p>

<h1 align="center">folio</h1>

<p align="center"><strong>Documents, sheets and slides in one file.</strong> (beta, Linux and macOS; Windows coming soon)<br/>
Native Rust app (GPUI) · opens and writes Word, Excel, PowerPoint and OpenDocument · drivable by your AI (MCP, CLI, built-in agent).<br/>
Part of <a href="https://lsuite.xyz">lsuite</a>, the free, open-source creative suite your AI can drive.</p>

---

folio is an office app built on one idea: a file holds any mix of **documents** (rich text on
pages), **sheets** (a grid with formulas) and **decks** (slides), and they work together. A table in
a report or a chart on a slide can show a sheet's range **live**: change a number in the sheet and
the report and the slides follow.

Coming from Microsoft Office, Google Workspace, Apple iWork or LibreOffice? folio opens `.docx`,
`.xlsx` and `.pptx` (what Google's apps download and what Pages, Numbers and Keynote export),
`.odt`, `.ods`, `.odp`, CSV and Markdown, and exports PDF, Office, OpenDocument, CSV, Markdown and
HTML. The look is lsuite's design system v2: black and white, square, with film grain behind the
chrome; the paper stays white.

## What it does

**Documents**
- Pages laid out exactly as they print (the window and the PDF share one layout engine)
- Paragraph styles (title, subtitle, three heading levels, quote, code, caption), bold, italic, underline, strikethrough, colour, highlight, size and font
- Bulleted, numbered and check lists with levels; alignment and justification
- Tables (Tab moves between cells and adds rows), pictures with captions, page breaks
- Page size and orientation, margins, headers and footers with page numbers
- Footnotes, comments with replies, tracked changes you accept or reject

**Sheets**
- A formula engine with references, ranges, other sheets (`'Raw data'!B2`), recalculation by dependencies and about 150 functions: SUM, AVERAGE, IF, IFS, VLOOKUP, XLOOKUP, INDEX, MATCH, COUNTIF(S), SUMIF(S), TEXT, DATE, EDATE, PMT, NPV…
- Number formats (currency, percent, dates, scientific…), bold, colours, fills, borders, alignment, wrap
- Sort, filter, fill down and right (series continue, formulas move their references), AutoSum
- Freeze the header, resize columns and rows, charts that sit over the grid

**Decks**
- Slide layouts (title, title and content, section, two columns, title only, blank) and themes
- Text boxes, rectangles, ellipses, triangles, lines and arrows; pictures; tables and charts
- Move and resize on the slide, speaker notes, hidden slides
- Present full screen, or with the presenter view (next slide, notes, clock)

**One file**
- Live tables and charts from any sheet range, in documents and on slides
- One undo history for everything, whoever made the change (you, the agent, a script)
- Saves itself; `.folio` is a zip of JSON and pictures ([docs/FILE_FORMAT.md](docs/FILE_FORMAT.md))

## Install

folio is in beta for **Linux** (x86_64; an AppImage or a `.deb`) and **macOS** (Apple Silicon and
Intel; a `.dmg`). **Windows is coming soon.**

Get folio in the [lsuite app](https://lsuite.xyz/launcher), which installs and updates the lsuite
apps; no account is needed. folio is free and open source: you can also build it from source
(below).

folio checks for updates through lsuite (`https://lsuite.xyz/api/apps/folio/latest.json`, no
account), verifies every download with the embedded release key, and stages the update for restart
(automatic installation is opt-in). The macOS app and the AppImage update in place with a rollback
copy; the `.deb` is updated by the lsuite app.

## Drive it from AI and scripts

Everything you can do in the window is a named command (`doc.write`, `sheet.setRange`,
`deck.addSlide`…, see [docs/COMMANDS.md](docs/COMMANDS.md)), and the window, the built-in agent,
`folio-cli` and `folio-mcp` all go through the same registry and share one undo history.

```bash
claude mcp add folio -- folio-mcp --live                                            # Claude Code
folio-cli file.overview                                                             # the running app
folio-cli --file plan.folio doc.write --markdown "# Plan\n\nShip on **Friday**."    # a file
folio-cli convert report.docx report.pdf
```

The **Agent** panel (Ctrl+J, ⌘J on macOS) runs the model you already have: Claude Code, Codex, an Anthropic,
OpenAI, OpenRouter, Gemini or Mistral key, or a local model (Ollama, LM Studio). Every agent, the built-in one or yours over MCP, gets folio's **harness**: an
office expert's brief, twelve skills (a report from notes, a budget, a deck from a document…), the
live state of the file before every step, pictures of the pages, slides and sheets it made
(`harness.look`) and objective checks (`harness.check`) it runs before saying it is done.
`folio-cli --file report.folio agent "…"` runs the built-in agent without the window, and
[`evals/`](evals) scores it on office jobs. Details: [docs/AI_CONTROL.md](docs/AI_CONTROL.md).

## Plugins

Ask the Agent panel for a **plugin** and it writes a spreadsheet function in Rust with folio's SDK
(`crates/folio-plugin`, see its [guide](crates/folio-plugin/GUIDE.md)), builds it and installs it.
The recipe agents follow is in [docs/AI_CONTROL.md](docs/AI_CONTROL.md#plugins-written-by-an-agent).
Two examples are in [`examples/plugins`](examples/plugins): finance-extra and text-tools.

## Works with the rest of lsuite

`handoff.apps` lists the lsuite apps on this computer (from `~/.lsuite/apps`). `handoff.image
app=nori` brings in nori's open image and `handoff.image app=kimchi time=12` a frame of kimchi's
cut, as pictures in a document or on a slide. See
[docs/AI_CONTROL.md](docs/AI_CONTROL.md#the-other-lsuite-apps).

## Documentation

- [docs/AI_CONTROL.md](docs/AI_CONTROL.md): the Agent panel, `folio-mcp`, `folio-cli`, permissions, plugins
- [docs/COMMANDS.md](docs/COMMANDS.md): every command
- [docs/FILE_FORMAT.md](docs/FILE_FORMAT.md): the `.folio` file
- [crates/folio-plugin/GUIDE.md](crates/folio-plugin/GUIDE.md): writing a plugin
- [CHANGELOG.md](CHANGELOG.md): what each release brought

## Architecture

```
crates/
  folio-calc      formula engine: parser, functions, number formats, dependency graph
  folio-core      the model: documents, sheets, decks, live links, one undo history, the .folio file
  folio-layout    text layout and pagination (cosmic-text), slide text, chart geometry, rasters
  folio-io        DOCX, XLSX, PPTX, ODF, CSV, Markdown, HTML and PDF
  folio-control   the command registry, session, permissions, loopback bridge, lsuite discovery, plugins
  folio-agent     the built-in agent
  folio-plugin    the plugin SDK (spreadsheet functions behind a frozen C ABI)
  folio-desktop   the window (GPUI), binary `folio`
  folio-cli       `folio-cli`: any command, on the running app or a file
  folio-mcp       `folio-mcp`: the registry as MCP tools (`--live`, `--file`)
  folio-release   signs update archives and writes latest.json for releases (no Node needed)
```

## Development

Rust 1.92 or later. Linux needs GPUI's usual libraries (the list is in
[`.github/workflows/ci.yml`](.github/workflows/ci.yml)).

```bash
cargo run -p folio                      # the window
cargo run -p folio-cli -- --help
cargo test --workspace
scripts/bundle-linux.sh                 # the AppImage and the .deb
scripts/bundle-macos.sh                 # folio.app, a .dmg and the updater archive (on a Mac;
                                        # aarch64-apple-darwin or x86_64-apple-darwin)
```

The Windows code and script stay in the source (`scripts/bundle-windows.sh`), but Windows isn't
built or shipped while folio is in beta: it is coming soon. CI builds macOS on the Mac mini (a
self-hosted runner) when started by hand: `gh workflow run CI --ref <branch>`.

### Signed releases

Linux and macOS. The suite's Apple credentials stay in the kimchi repository. Maintainers dispatch
`ludovic111/kimchi`'s `suite-build.yml` with `app=folio` and the full, published commit SHA, then
publish the run's signed Linux and macOS artifacts to lsuite's build store with
`scripts/publish-build.sh X.Y.Z <run-id>` (it needs the run's Linux and both macOS jobs to have
succeeded, takes the Linux and macOS files, checks every signature, writes `latest.json` with
`folio-release manifest` and `SHA256SUMS`, and creates `folio-vX.Y.Z` in `ludovic111/lsuite-builds`;
`DRY_RUN=1` stops before publishing). `.github/workflows/release.yml` makes a draft release instead,
published the same way with `scripts/publish-build.sh X.Y.Z`. Keep private signing keys outside the
repository.

## License

MIT. Fonts: IBM Plex and Chakra Petch (SIL Open Font License). Icons: Lucide (ISC). Other apps'
logos belong to their owners ([`crates/folio-desktop/assets/logos/SOURCES.md`](crates/folio-desktop/assets/logos/SOURCES.md)).

folio is free. If it helps you, [support it](https://lsuite.xyz/folio/support).
