# Letter or CV
When: a formal letter (cover letter, complaint, request, resignation) or a CV / résumé on one or two pages.

## Steps: letter
1. Gather: sender (name, address, email), recipient, date, subject, the one thing the letter asks or says. Ask only for what can't be left as a clear placeholder like [Your address].
2. `page.add kind=doc name="Letter"` (or the empty page shown). `doc.setup size=a4 margins=70 footer=""` (Letter size in the US).
3. `doc.write replace=true`: sender block (each line its own line), blank line, date, recipient block, **Subject: …** (bold), salutation, three short paragraphs (why you write, the substance, what you ask and by when), sign-off and name. No headings in a letter.
4. Keep it to one page.

## Steps: CV
1. Gather roles (title, employer, dates, two to four achievements each with a number where possible), education, skills, languages, contact.
2. `doc.setup margins=50 footer=""`. `doc.write replace=true`: the name as `# Title`, one contact line, a two-line profile, then `## Experience` (each role as `### Title · Employer · 2022–2026` then bullets starting with a verb), `## Education`, `## Skills`.
3. Reverse chronological order; the same date format everywhere; no photo or age unless asked.

## Checks
- `doc.stats`: a letter fits on 1 printed page; a CV on 1–2.
- `harness.look page=… pageNumber=1`: blocks aligned, nothing runs onto a second page by a line.
- No placeholder left that the person gave you; placeholders you had to leave are listed in your reply.
