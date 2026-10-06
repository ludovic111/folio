# Driving folio from an AI agent or a script

Everything a person can do in the folio window is a named command in one registry
(`family.verb`, JSON parameters in, JSON result out). The window, the built-in agent (the Agent
panel, ⌘J), `folio-cli` and `folio-mcp` all run the same commands, with the same checks and one
undo history. The full list, generated from the registry, is [COMMANDS.md](COMMANDS.md).

## Four ways in

| Client | How |
| --- | --- |
| The Agent panel | ⌘J in the window. lsuite AI works without setup once signed in; Claude Code, Codex, API keys (Anthropic, OpenAI, OpenRouter, Mistral) and local models (Ollama, LM Studio) work too. |
| MCP | `claude mcp add folio -- /Applications/folio.app/Contents/MacOS/folio-mcp --live` drives the running app; `folio-mcp --file report.folio` works on a file without it. `folio-cli mcp-config` prints the lines for Claude Code, Codex, Cursor and Claude Desktop. |
| CLI | `folio-cli <command> --param value` on the running app, or `folio-cli --file report.folio <command>` on a file. `folio-cli batch` runs JSON lines from stdin. `folio-cli convert in.docx out.pdf`. |
| The bridge | The app listens on 127.0.0.1 only and writes `{port, token}` to `<data>/control.json` (0600). Clients send newline-delimited JSON-RPC: first `auth` with the token, then any command. `~/.lsuite/apps/folio.json` says where everything is. |

## Read first

`file.overview` returns the whole file in one bounded answer: every page with what it holds
(documents: outline, words, first paragraphs; sheets: used range, sample values, formulas and
errors; decks: each slide's title and text), live links between pages, the undo history and what
the window shows (the page, the caret, the selected cells, the slide).

## Conventions

- **Pages** are named by id, name or 1-based number (`page: "Budget"`). Without `page`, a command
  uses the page the window shows when it has the right kind, else the first page of that kind.
- **Documents**: blocks are counted from 0 (`doc.read` lists them with ids). Positions are
  `{"block": 2, "offset": 5}` with offsets in characters, plus `"cell": [row, col]` inside a table.
  The quickest way to write is `doc.write` with Markdown (headings, lists, **bold**, tables, `---`
  for a page break). `doc.format find="word"` formats text without positions.
- **Sheets**: cells and ranges in A1 notation (`B2`, `B2:D9`, `B:B`, `'Raw data'!A1`).
  `sheet.setRange` takes rows of values; anything starting with `=` is a formula, with Excel's
  syntax and functions (`sheet.functions` lists them). Answers list cells that show an error, so
  mistakes are seen at once. `sheet.evaluate` computes a formula without writing it.
- **Decks**: slides by id or 1-based number; shapes by id or name. Coordinates are points on a
  960 × 540 slide. `deck.addSlide layout=titleContent title=… body="one\ntwo"` makes bullets.
- **Live links**: `doc.insertTable link='Numbers'!A1:E5`, `doc.insertChart source=…`,
  `deck.addChart source=…`, `deck.addTable link=…` show a sheet range as it is computed now, and
  follow every change.
- **Undo**: every command that changes the file is one undo step (`history.undo` undoes the last
  step, whoever made it). `file.batch` runs several commands as one step and rolls back if one
  fails. Typing-like edits passing the same `coalesce` key within about a second fold into one step.
- The file saves itself a moment after every change; `file.export` writes PDF, DOCX, XLSX, PPTX,
  ODT, ODS, ODP, CSV, Markdown or HTML.

## Permissions

Agents (the built-in one, MCP clients, `folio-cli --agent`) may always read and edit the open file.
Settings › Agent permissions decide whether they may also open, import and export files, change
settings, build and install plugins, or control the app and other lsuite apps. API keys, the
agent provider and the permissions themselves stay with the person. Off means off for every agent.

## Plugins written by an agent

`plugin.guide` explains folio's plugin SDK (spreadsheet functions in Rust). The recipe:
`plugin.toolchain` → `plugin.new` → `plugin.writeSource` → `plugin.build` until green →
`plugin.publishLocal` → try it with `sheet.evaluate`. Needs the plugins permission.

## The other lsuite apps

`handoff.apps` lists the lsuite apps on this computer (from `~/.lsuite/apps`). `handoff.image
app=nori` brings in nori's open image and `handoff.image app=kimchi time=12` a frame of kimchi's
cut, as pictures in a document or on a slide.
