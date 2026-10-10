# Changelog

What changed in each folio release.

## Unreleased

lsuite is now entirely free: no account, no subscription.

### Removed
- **lsuite AI and the lsuite account.** The agent runs on what you bring: Claude Code, Codex, an Anthropic, OpenAI, OpenRouter, Gemini or Mistral key, or a local model (Ollama, LM Studio, any OpenAI-compatible server). The `account.*` commands, the lsuite AI dialog, the sign-in in the Agent panel and in the first-run setup are gone. Settings that chose lsuite AI switch to Claude Code; an old `~/.lsuite/account.json` is left alone and ignored.

### Changed
- **Updates without an account.** The update check reads `https://lsuite.xyz/api/apps/folio/latest.json` (or `$LSUITE_SERVER`), which is public, and downloads through lsuite without a token; signatures are checked exactly as before. Nobody is asked to sign in any more.
- The first-run setup offers Claude Code (the default), Codex, an API key, Ollama or no agent.

## 0.2.0 — 2026-10-07 (beta)

folio's agent harness: an agent working in folio knows office work, sees what it made and checks it before it says it is done, whether it is the Agent panel, `folio-cli agent` or an outside agent over `folio-mcp`.

**folio is in beta for Linux** (x86_64: AppImage and `.deb`, through the lsuite app). macOS and Windows are coming soon: this release and its updates are Linux only.

### New
- **An expert brief.** The built-in agent and `folio-mcp` share one brief (`harness.brief`): the file's model, the quality bar for documents (structure is styles, citations, page setup), sheets (inputs, calculations, outputs; formats; charts that say one thing; formula errors) and decks (one idea per slide, takeaway titles, text that fits), live links, the usual mistakes and a finish routine.
- **Skills.** Twelve playbooks for office jobs, each with the steps, the exact commands and the checks: a report from notes, a letter or CV, meeting minutes, reviewing a document, a budget or model sheet, cleaning data and summarising it, a chart from data, fixing formula errors, a deck from a document, a pitch deck, importing and converting Office files, writing a function plugin. `harness.skills` lists them, `harness.skill` loads one; over MCP each is also a prompt (`skill-<name>`) and a resource (`folio://skills/<name>`).
- **Live context before every step.** The agent gets the file, each page's size, what the window shows and selects, the open problems and what you changed since its last step, before every model step, not only with your request (`harness.context`; `folio-mcp` adds it to tool results when it changed, with a reminder of the finish routine after an edit until the agent checks, in the text and under `harnessNotes` in the structured result, which is what Claude Code reads).
- **The agent sees its work.** `harness.look` draws a document's printed page, a slide, or a sheet range with its charts, exactly as the window and the PDF do, with its numbers (pages, words, headings, slide text and overflow, column sums, errors). The picture reaches models that can see, in the Agent panel (lsuite AI, Anthropic, OpenAI, OpenRouter, Gemini, Mistral, vision models on Ollama and LM Studio) and over MCP.
- **Objective checks.** `harness.check` finds formula errors (the cell that causes them first), totals that leave out rows, empty cells inside tables and summed ranges, columns too narrow for their numbers, skipped heading levels, bold lines posing as headings, placeholder text, text overflowing slide boxes, shapes off the slide, empty titles and broken live links.
- **`folio-cli agent`.** Runs the built-in agent on a file without the window (`folio-cli --file report.folio agent "…" --provider claude-code`).
- **Evals.** Eleven scripted office jobs in `evals/`, run headless with a real model and scored automatically on the resulting file (`python3 evals/run.py`; results in `evals/RESULTS.md`): 11 of 11 pass with Opus.

### Changed
- **Updates come through lsuite.** The update check asks lsuite.xyz with your lsuite account (the free account the lsuite app signs in), and downloads through it; signatures are checked exactly as before. Signed out, it says to sign in in the lsuite app. `scripts/publish-build.sh` publishes a build to lsuite's build store.
- Commands that answer Markdown (the brief, a skill, the plugin guide) reach agents as text, not as a JSON string.
- A page or slide named by its number can be given as a number (`slide: 2`) as well as text, in every command.

## 0.1.0 — 2026-10-07 (beta)

The first release of folio, lsuite's office app.

### New
- **One file, three kinds of page.** A `.folio` file holds any mix of documents, sheets and decks, listed in the Pages sidebar. Add, rename, reorder and duplicate pages; open the file anywhere, it is a zip of JSON and pictures (docs/FILE_FORMAT.md).
- **Documents.** Pages of paper laid out exactly as they print: paragraph styles (title, headings, quote, code…), bold, italic, underline, strikethrough, colours, sizes and fonts, lists and checklists, tables, pictures, page breaks, headers and footers with page numbers, page size and margins, footnotes, comments and tracked changes.
- **Sheets.** A grid with a real formula engine: references, ranges, other sheets, recalculation by dependencies, and the functions people use (SUM, AVERAGE, IF, VLOOKUP, XLOOKUP, INDEX, MATCH, COUNTIF, SUMIF, TEXT, DATE and many more). Number formats, sorting, filters, fill down and right, frozen rows, column widths, charts.
- **Decks.** Slides with layouts, text boxes, shapes, pictures, tables and charts; move and resize them on the slide, speaker notes, themes, and presenting full screen or with the presenter view.
- **Live links.** A table in a document, a chart on a slide or in a report can show a sheet's range as it is computed now, and follows every change.
- **Coming from another suite.** folio opens Word, Excel and PowerPoint files (also what Google Docs, Sheets and Slides download and what Pages, Numbers and Keynote export), OpenDocument files, CSV and Markdown, and exports to PDF, Word, Excel, PowerPoint, OpenDocument, CSV, Markdown and HTML.
- **Your AI can drive it.** Every action is a command shared by the window, the Agent panel (⌘J), `folio-cli` and `folio-mcp`, with one undo history. lsuite AI works without setup once you sign in; Claude Code, Codex, API keys and local models work too.
- **Plugins.** Spreadsheet functions written in Rust with folio's plugin SDK; ask your agent to build one and it appears without a restart.
- **lsuite.** folio lists itself in `~/.lsuite/apps/folio.json` for the other apps, and takes pictures from nori and frames from kimchi.
