# folio eval results

Scripted office jobs (`evals/jobs.py`) run headless by folio's built-in agent with a real model
(`python3 evals/run.py`), each scored by automatic checks on the resulting file. A job passes when
the agent finished and every check passed. "Looked" and "Checked" count the agent's
`harness.look` and `harness.check` calls (the finish routine); "Skill" says whether it loaded one.
Newest runs at the bottom.

## 2026-10-08 · claude-code · default · 2/2 jobs passed · checks 100%

Run `evals/results/2026-10-08-024224`.

| Job | Result | Checks | Commands | Looked | Checked | Skill | Seconds | Failed checks |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| budget-sheet | pass | 8/8 | 4 | 1 | 1 | no | 13.3 | — |
| meeting-minutes | pass | 8/8 | 16 | 2 | 1 | yes | 33.1 | — |

## 2026-10-08 · claude-code · default · 2/2 jobs passed · checks 100%

Run `evals/results/2026-10-08-030000`.

| Job | Result | Checks | Commands | Looked | Checked | Skill | Seconds | Failed checks |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| budget-sheet | pass | 8/8 | 4 | 1 | 1 | no | 13.4 | — |
| meeting-minutes | pass | 8/8 | 9 | 2 | 2 | yes | 22.4 | — |

## 2026-10-08 · claude-code · opus · 9/11 jobs passed · checks 97%

Run `evals/results/2026-10-08-120251`.

| Job | Result | Checks | Commands | Looked | Checked | Skill | Seconds | Failed checks |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| report-from-notes | pass | 8/8 | 11 | 2 | 1 | no | 41.5 | — |
| cover-letter | pass | 7/7 | 10 | 2 | 1 | yes | 34.8 | — |
| budget-sheet | pass | 8/8 | 4 | 1 | 1 | no | 15.5 | — |
| clean-and-summarise | pass | 4/4 | 6 | 1 | 1 | no | 29.9 | — |
| chart-from-data | pass | 6/6 | 6 | 1 | 1 | no | 12.3 | — |
| deck-from-document | fail | 5/6 | 21 | 5 | 3 | yes | 46.5 | the slides render (CliError: harness.look {'page': 'm4pn7j27', 'slide': 1}: `slide` should be a str) |
| pitch-deck | fail | 6/7 | 28 | 6 | 1 | yes | 76.0 | the slides render (CliError: harness.look {'page': 'wwzu2i38', 'slide': 1}: `slide` should be a str) |
| meeting-minutes | pass | 8/8 | 14 | 2 | 3 | yes | 27.0 | — |
| fix-formula-errors | pass | 5/5 | 9 | 1 | 2 | no | 26.9 | — |
| import-csv | pass | 4/4 | 8 | 1 | 1 | no | 15.5 | — |
| linked-report | pass | 4/4 | 8 | 1 | 1 | no | 12.0 | — |

## 2026-10-08 · claude-code · opus · 2/2 jobs passed · checks 100%

Run `evals/results/2026-10-08-121624`.

| Job | Result | Checks | Commands | Looked | Checked | Skill | Seconds | Failed checks |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| deck-from-document | pass | 6/6 | 19 | 3 | 3 | yes | 43.4 | — |
| pitch-deck | pass | 7/7 | 21 | 5 | 3 | yes | 83.4 | — |

**Release run for 0.2.0 (Opus): 11/11.** The two deck jobs in the full run above failed in
scoring, not in the agent's work: `harness.look` refused `slide: 1` as a JSON number although the
parameter says "its 1-based number" (the scorer sent one, and so could an agent). Every string
parameter now takes a number as its text (`registry::coerce`; test `slides_and_pages_by_number`),
and both jobs were re-run with the fix (this run): 6/6 and 7/7. Every job ran `harness.check` and
`harness.look` before finishing.
