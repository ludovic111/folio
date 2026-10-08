# Meeting minutes
When: notes or a transcript of a meeting to turn into minutes with decisions and actions.

## Steps
1. Read the notes; extract: meeting name, date, attendees (and apologies), agenda items, what was discussed, decisions, actions (who, what, by when), open questions, next meeting.
2. `page.add kind=doc name="Minutes <date>"` (or the empty page shown), then one `doc.write replace=true`:
   `# <Meeting> — <date>`; a line "Attendees: …"; `## Decisions` (a numbered list, each a full sentence); `## Actions` (a Markdown table | Action | Owner | Due |); `## Discussion` (Heading 3 per agenda item, two to four bullets each); `## Open questions`; `## Next meeting`.
3. Dates in one format (2026-10-07 or 7 Oct 2026); owners by name, never "someone".
4. Checklist style for actions if the person prefers (`- [ ] …`).

## Checks
- Every action has an owner and a due date (or "no date" said explicitly); every decision in the notes is listed once.
- `doc.outline` shows the sections in order; `harness.check` is clean; `harness.look pageNumber=1` reads like minutes.
