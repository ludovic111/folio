# Pitch deck
When: a deck to convince (investors, a client, a manager) from a short brief about a product, a project or a company.

## Steps
1. Gather: what it is, for whom, the problem, the evidence (numbers), the ask. Don't invent traction or figures: leave clear placeholders like [MRR] and list them in your reply.
2. The classic arc, one idea per slide, 10–12 slides: Title (name + one-line promise) · Problem · Solution · How it works / product · Market · Traction (a chart) · Business model · Competition (twoContent or a table) · Team · Plan and milestones · The ask · Thank you / contact.
3. `page.add kind=deck name="Pitch"`, `deck.setTheme`, then one `file.batch` of `deck.addSlide` calls with takeaway titles ("Teams lose 6 hours a week to …"), at most four bullets, and notes with the spoken pitch.
4. Numbers in a sheet page (`page.add kind=sheet name="Metrics"`, `sheet.setRange`), shown live with `deck.addChart source='Metrics'!A1:B7 kind=line` on titleOnly slides.
5. Large type, little text: a slide with one big number (`deck.addShape kind=text textSize=60`) beats a bullet list.

## Checks
- `harness.check page=Pitch` is clean (no overflow, every slide titled).
- `harness.look` the title, traction and ask slides.
- Read the titles in order: they tell the pitch on their own.
