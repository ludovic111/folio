# Deck from a document
When: turn a report, a plan or notes already in the file into slides.

## Steps
1. `doc.outline` and `doc.read markdown=true` the document. Each Heading 1 is a candidate section; each key finding a candidate slide. Aim for one slide per idea, 6–12 slides for a typical report.
2. Write the storyline first: the slide titles alone, as takeaway sentences, should tell the story.
3. `page.add kind=deck name="Slides"`, `deck.setTheme theme=paper` (or what suits). Then in one `file.batch`:
   - `deck.addSlide layout=title title=… body="subtitle · date"`;
   - per section, `layout=section` when the deck has several parts;
   - content slides `layout=titleContent` with at most six short bullets (`body="one\ntwo\nthree"`) and `notes=` holding what to say (the document's sentences belong in notes, not on slides);
   - comparisons on `twoContent` (a blank line between the columns);
   - a closing slide: the recommendation or next steps.
4. Numbers from a sheet: `deck.addChart` or `deck.addTable link=…` on a `titleOnly` slide, live.
5. Remove the empty first slide a new deck may have if you didn't use it (`deck.read` shows it).

## Checks
- `harness.check page=Slides`: no overflowing text, no slide without a title, no leftover empty placeholder.
- `harness.look page=Slides slide=N` for the title slide, one content slide and every chart slide.
- The deck covers every Heading 1 of the document, and nothing contradicts it.
