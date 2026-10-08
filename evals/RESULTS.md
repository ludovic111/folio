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
