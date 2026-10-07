# Review a document
When: the person asks for feedback, proofreading or edits on a document they wrote.

## Steps
1. `doc.read markdown=true` and `doc.outline`. Read it once for structure and argument, once for wording.
2. Structure first: is there one Title and a logical heading order? Fake headings (bold Normal lines) become real ones only if the person asked for edits.
3. For decisions that belong to the author (an unclear claim, a missing source, a section to cut), leave a comment: `doc.comment find="the passage" text="…"`. One comment per point, specific and kind.
4. For wording you are sure of (typos, grammar, repetition), turn tracked changes on (`doc.trackChanges on=true`) and edit with `doc.replace find=… replace=…`, so the author accepts or rejects each change. Don't rewrite their voice.
5. Facts without a source get a comment asking for one; never add a source yourself unless the person gave it.

## Checks
- `doc.comments` lists your comments on the right passages; tracked changes are on if you edited.
- `harness.check page=…` is clean (no heading level skipped, no empty heading).
- Your reply summarises the main findings in three to five lines.
