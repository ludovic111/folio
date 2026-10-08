# Driving folio from an AI agent or a script

Everything a person can do in the folio window is a named command in one registry
(`family.verb`, JSON parameters in, JSON result out). The window, the built-in agent (the Agent
panel, Ctrl+J), `folio-cli` and `folio-mcp` all run the same commands, with the same checks and one
undo history. The full list, generated from the registry, is [COMMANDS.md](COMMANDS.md).

## Four ways in

| Client | How |
| --- | --- |
| The Agent panel | Ctrl+J in the window. lsuite AI works without setup once signed in; Claude Code, Codex, API keys (Anthropic, OpenAI, OpenRouter, Mistral) and local models (Ollama, LM Studio) work too. |
| MCP | `claude mcp add folio -- folio-mcp --live` drives the running app; `folio-mcp --file report.folio` works on a file without it. `folio-cli mcp-config` prints the lines for Claude Code, Codex, Cursor and Claude Desktop. |
| CLI | `folio-cli <command> --param value` on the running app, or `folio-cli --file report.folio <command>` on a file. `folio-cli batch` runs JSON lines from stdin. `folio-cli convert in.docx out.pdf`. `folio-cli --file report.folio agent "…" [--provider claude-code] [--model …] [--json]` runs the built-in agent on a file without the window. |
| The bridge | The app listens on 127.0.0.1 only and writes `{port, token}` to `<data>/control.json` (0600). Clients send newline-delimited JSON-RPC: first `auth` with the token, then any command. `~/.lsuite/apps/folio.json` says where everything is. |

## Read first

`file.overview` returns the whole file in one bounded answer: every page with what it holds
(documents: outline, words, first paragraphs; sheets: used range, sample values, formulas and
errors; decks: each slide's title and text), live links between pages, the undo history and what
the window shows (the page, the caret, the selected cells, the slide).

## The harness: how an agent does office work well

lsuite's HARNESS.md, met the same way for the Agent panel, `folio-cli agent` and any MCP client.
The sources are in `crates/folio-control/src/harness/` (`brief.md`, `skills/*.md`).

| Command | Does |
| --- | --- |
| `harness.brief` | The expert brief (Markdown, about 1,500 words): the file's model, the quality bar for documents, sheets and decks, live links, the usual mistakes, the finish routine and the skills' index. It is the built-in agent's system prompt and `folio-mcp`'s `instructions`. |
| `harness.skills` | `[{name, title, when}]`: twelve playbooks (report-from-notes, letter-or-cv, meeting-minutes, review-document, budget-model, clean-and-summarise, chart-from-data, fix-formula-errors, deck-from-document, pitch-deck, import-convert, function-plugin). |
| `harness.skill name=…` | One playbook: when to use it, the steps with the exact commands, the checks. |
| `harness.context since=…` | The live context: the file and each page's size, what the window shows and selects, open problems, and what the person changed since the step `since` (`seq` in the answer). |
| `harness.look` | A picture of a document's printed page (`page`, `pageNumber`), a slide (`slide`) or a sheet range with its charts (`range`), drawn as the window and the PDF draw it, written to `<data>/looks/` (its `path` is in the answer), with its numbers and the page's problems. |
| `harness.check` | Objective checks on the file or one `page`: errors (formula errors, text overflowing a slide box, shapes off the slide, broken live links) and warnings (totals that leave out rows, empty cells in tables and summed ranges, narrow columns, skipped heading levels, bold lines posing as headings, placeholder text, empty titles and placeholders, too many bullets). `ok` is true when there are no errors. |

- **Live context every step.** The built-in agent sends `harness.context` with the request and
  again after each step's results when it changed. `folio-mcp` adds a `<context>` block to a tool
  result when the context changed since the last one it gave (with what the person changed
  meanwhile), the path an edit was saved to in file mode, and, after an edit, a reminder of the
  finish routine until the agent runs `harness.check` or `harness.look`. These notes end the
  result's text and are also the list `harnessNotes` in its `structuredContent`, because some
  clients (Claude Code) show the structured result instead of the text. A result with a picture
  has no `structuredContent`, so the picture is never hidden behind it.
- **Pictures.** `harness.look` and `ui.screenshot` answers carry a picture: the built-in agent
  attaches it to the tool result for models that can see (Anthropic and lsuite AI inside the
  result, OpenAI-compatible servers and Ollama in a message after it, Gemini inline; a server that
  refuses pictures gets the request again without them), and `folio-mcp` returns it as MCP image
  content after the text.
- **Finish routine.** Before saying it is done the agent runs `harness.check`, looks at what it
  made with `harness.look`, fixes what is off (three passes at most) and reports in a few lines.
- **MCP.** The brief is `instructions` and the resource `folio://harness/brief`; each skill is a
  prompt `skill-<name>` (with an optional `request`) and a resource `folio://skills/<name>`;
  `folio://harness/context` is the live context.
- **One undo per turn.** The agent takes a checkpoint before its first change; "Revert this run"
  (or `agent.revert`) undoes the whole turn, and the panel lists every change.
- **Evals.** `python3 evals/run.py` runs eleven office jobs with a real model (Claude Code by
  default, no key needed) and scores the files; `evals/RESULTS.md` keeps the pass rates.

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
