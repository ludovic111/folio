"""The eval jobs: scripted office work, each scored by automatic checks on the resulting file.

A job starts from a new file (`start`: its first page) and a fixture (`setup`: commands run
before the agent), gives the agent one request, then checks the file. Checks read the file only
(structure, numbers, harness.check, looks), never the agent's reply.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Callable

from folio import Checks, Folio, close, number


@dataclass
class Job:
    name: str
    title: str
    prompt: str
    score: Callable[[Folio, Checks], None]
    start: str = "doc"
    setup: list[tuple[str, dict]] = field(default_factory=list)
    skill: str = ""


# ---- shared checks --------------------------------------------------------------------------


def no_errors(f: Folio, c: Checks, page: str | None = None) -> None:
    def run():
        r = f.check(page)
        errs = [p["message"] for p in r["problems"] if p["severity"] == "error"]
        return (not errs, "; ".join(errs[:3]) or "clean")

    c.run("harness.check finds no errors" + (f" on {page}" if page else ""), run)


def headings(f: Folio, page: str) -> list[tuple[int, str]]:
    out = []
    for b in f.doc_blocks(page):
        style = (b.get("style") or "").lower()
        level = {"title": 0, "heading1": 1, "heading2": 2, "heading3": 3}.get(style)
        if level is not None:
            out.append((level, b.get("text", "")))
    return out


def doc_page(f: Folio) -> str:
    docs = f.pages_of("doc")
    if not docs:
        raise AssertionError("no document page")
    # The longest one: the agent may have added a page rather than writing in the first.
    best = max(docs, key=lambda p: len(f.call("page.text", page=p["id"])["text"]))
    return best["id"]


def has_table(f: Folio, page: str) -> bool:
    return any(b.get("kind") == "table" or b.get("type") == "table" for b in f.doc_blocks(page))


def text_of(f: Folio, page: str) -> str:
    return f.call("page.text", page=page)["text"]


def deck_page(f: Folio) -> dict:
    decks = f.pages_of("deck")
    if not decks:
        raise AssertionError("no deck page")
    return max((f.deck(d["id"]) | {"id": d["id"]} for d in decks), key=lambda d: len(d["slides"]))


def slide_title(s: dict) -> str:
    for sh in s.get("shapes", []):
        if sh.get("placeholder") == "title" or (sh.get("name") or "").lower() == "title":
            return sh.get("text", "")
    return ""


def formula_cells(cells: dict) -> dict:
    return {a: c for a, c in cells.items() if str(c.get("input", "")).startswith("=")}


def sheet_named(f: Folio, *names: str) -> dict | None:
    for p in f.pages_of("sheet"):
        if p["name"].lower() in [n.lower() for n in names]:
            return p
    return None


def find_value(cells: dict, target: float, formulas_only: bool = True) -> str | None:
    """The address of a cell holding `target` (a formula's result by default)."""
    for a, cell in cells.items():
        if formulas_only and not str(cell.get("input", "")).startswith("="):
            continue
        if close(number(cell.get("value")), target):
            return a
    return None


# ---- the jobs -------------------------------------------------------------------------------

NOTES = """Notes from the Q3 review of the Lyon bakery chain (for the board):
- revenue Q3: 1.84 M EUR, up 12% on Q2 (1.64 M); best month August
- 3 shops: Croix-Rousse 720k, Part-Dieu 610k, Confluence 510k
- costs up 7%: flour +18% since June, staff +4% (two new bakers)
- margin 14.2% (Q2: 13.1%)
- customer survey: 4.6/5, complaints about queues at Part-Dieu at lunch
- options: second oven at Part-Dieu (45k EUR), click-and-collect app (30k EUR), both
- recommendation: oven first (payback ~9 months), app in Q1 if Q4 margin stays above 13%
- next steps: order the oven by 15 Nov, quote for the app, board decision 30 Oct"""


def score_report(f: Folio, c: Checks) -> None:
    page = doc_page(f)
    hs = headings(f, page)
    c.add("has a title", any(l == 0 for l, _ in hs) or (hs and hs[0][0] == 1), str(hs[:3]))
    c.add("three or more section headings", sum(1 for l, _ in hs if l in (1, 2)) >= 3, f"{len(hs)} headings")
    c.add("no skipped heading level", all(p["kind"] != "headingSkip" for p in f.check(page)["problems"]))
    c.add("a table compares the shops", has_table(f, page))
    text = text_of(f, page).lower()
    facts = ["1.84", "12", "14.2", "45", "30", "croix-rousse", "part-dieu", "confluence", "oven"]
    missing = [x for x in facts if x not in text]
    c.add("keeps the facts of the notes", not missing, f"missing {missing}" if missing else "all there")
    c.add("a summary near the top", "summary" in " ".join(t.lower() for _, t in hs[:3]) or "summary" in text[:600])
    no_errors(f, c)
    c.run("the first page renders", lambda: f.look(page=page)["printedPages"] >= 1)


def score_letter(f: Folio, c: Checks) -> None:
    page = doc_page(f)
    stats = f.call("doc.stats", page=page)
    pages = stats.get("pages") or stats.get("printedPages")
    c.add("fits on one printed page", pages == 1, f"{pages} pages")
    text = text_of(f, page)
    for what in ["Marion Lefèvre", "Atelier Nord", "Product Designer"]:
        c.add(f"names {what}", what.lower() in text.lower())
    c.add("has a salutation and a sign-off", ("dear" in text.lower()) and any(w in text.lower() for w in ["sincerely", "regards", "best wishes"]))
    c.add("no placeholder left", all(p["kind"] != "placeholder" for p in f.check(page)["problems"]))
    c.add("not a wall of headings", sum(1 for l, _ in headings(f, page) if l >= 1) <= 1)


def score_budget(f: Folio, c: Checks) -> None:
    sheets = f.pages_of("sheet")
    c.add("a sheet page", bool(sheets))
    if not sheets:
        return
    sheet = sheets[0]["id"]
    cells = f.cells(sheet)
    monthly = 1150 + 420 + 95 + 60 + 180 + 75
    c.add("monthly total is a formula equal to 1,980", find_value(cells, monthly) is not None)
    c.add("yearly total is a formula equal to 23,760", find_value(cells, monthly * 12) is not None)
    c.add("uses formulas (8 or more)", len(formula_cells(cells)) >= 8, f"{len(formula_cells(cells))} formulas")
    c.add("numbers are formatted", any(c2.get("format", {}).get("number") for c2 in cells.values()))
    c.add("header is bold", any(c2.get("format", {}).get("bold") for c2 in cells.values()))
    no_errors(f, c)
    c.run("the sheet renders", lambda: f.look(page=sheet)["kind"] == "sheet")


SALES_ROWS = [
    ["Date", "Region", "Rep", "Amount"],
    ["2026-09-01", "North", "Ana", "1200"],
    ["2026-09-02", "north ", "Ben", "1 050"],
    ["2026-09-02", "South", "Cara", "980"],
    ["2026-09-03", "SOUTH", "Ana", "1,340"],
    ["2026-09-04", "East", "Ben", "760"],
    ["", "", "", ""],
    ["2026-09-05", "East", "Cara", "910"],
    ["2026-09-06", "North", "Dev", "1 500"],
    ["2026-09-07", "South", "Dev", "620"],
    ["2026-09-08", "east", "Ana", "1,180"],
]
SALES_BY_REGION = {"north": 1200 + 1050 + 1500, "south": 980 + 1340 + 620, "east": 760 + 910 + 1180}


def score_clean(f: Folio, c: Checks) -> None:
    sheets = f.pages_of("sheet")
    all_cells = {p["name"]: f.cells(p["id"]) for p in sheets}
    found = {}
    for name, cells in all_cells.items():
        for region, total in SALES_BY_REGION.items():
            at = find_value(cells, total)
            if at:
                found[region] = f"{name}!{at}"
    c.add("a total per region, computed by formulas", len(found) == 3, str(found))
    grand = sum(SALES_BY_REGION.values())
    c.add("the grand total is right (9,540)", any(find_value(cells, grand) for cells in all_cells.values()))
    # The cleaned data: numbers stored as numbers.
    numeric = 0
    for cells in all_cells.values():
        for a, cell in cells.items():
            if isinstance(cell.get("value"), (int, float)) or (isinstance(cell.get("value"), dict) and "number" in cell.get("value", {})):
                numeric += 1
    c.add("amounts are numbers, not text", numeric >= 10, f"{numeric} numeric cells")
    no_errors(f, c)


SALES_SETUP = [("sheet.setRange", {"page": "Sales", "at": "A1", "values": [
    ["Month", "North", "South"],
    ["Jan", 120, 80], ["Feb", 135, 95], ["Mar", 150, 70], ["Apr", 160, 110], ["May", 172, 118], ["Jun", 181, 124],
    ["Total", "=SUM(B2:B7)", "=SUM(C2:C7)"],
]})]


def score_chart(f: Folio, c: Checks) -> None:
    sheet = sheet_named(f, "Sales")
    charts = f.page(sheet["id"]).get("charts", []) if sheet else []
    links = f.call("link.list")
    sources = [ch["chart"]["source"] for ch in charts] + [l["link"] for l in links]
    c.add("a chart reads the Sales data", any("A1" in s.upper() or "B1" in s.upper() or "A2" in s.upper() for s in sources), str(sources))
    c.add("the chart leaves out the totals row", sources and all(not s.upper().endswith("8") for s in sources), str(sources))
    titles = [ch["chart"].get("title", "") for ch in charts]
    c.add("its title says something (not 'Chart')", any(len(t.split()) >= 3 for t in titles), str(titles))
    kinds = [ch["chart"].get("kind") for ch in charts]
    c.add("a line or column chart for months", any(k in ("line", "column", "area") for k in kinds), str(kinds))
    c.add("live links resolve", all(l["ok"] for l in links))
    no_errors(f, c)


REPORT_SETUP = [("doc.write", {"page": "1", "replace": True, "markdown": """# Moving the support team to four-day weeks

## Summary

A six-month trial of a four-day week (32 hours, same pay) for the 14-person support team cut the time to first answer from 5.1 to 3.8 hours and raised satisfaction from 4.1 to 4.4 out of 5. Sick days fell by 31%. We recommend making it permanent from January and trying it in sales next.

## Background

Support answered 2,300 tickets a month with growing delays. Burnout came up in every exit interview in 2025.

## What we tried

From March to August the team worked Monday to Thursday, with a rota covering Fridays. Ticket routing was simplified and meetings were cut to two a week.

## Results

| Measure | Before | After |
| --- | --- | --- |
| First answer (hours) | 5.1 | 3.8 |
| Satisfaction (out of 5) | 4.1 | 4.4 |
| Sick days per month | 13 | 9 |
| Tickets per month | 2,300 | 2,350 |

## Risks

Friday cover depends on two people; holidays need planning. Some customers expect same-day answers on Fridays.

## Recommendation

Make the four-day week permanent for support from January, hire one more person for Friday cover, and run the same trial in sales from April.
"""})]


def score_deck(f: Folio, c: Checks, lo: int, hi: int, chart: bool = False) -> None:
    d = deck_page(f)
    slides = d["slides"]
    c.add(f"{lo} to {hi} slides", lo <= len(slides) <= hi, f"{len(slides)} slides")
    titled = [s for s in slides if slide_title(s).strip()]
    c.add("every slide has a title", len(titled) == len(slides), f"{len(titled)}/{len(slides)}")
    long_titles = [slide_title(s) for s in slides if len(slide_title(s).split()) >= 4]
    c.add("titles say takeaways (most have 4+ words)", len(long_titles) * 2 >= len(slides), f"{len(long_titles)}/{len(slides)}")
    noted = [s for s in slides if s.get("notes")]
    c.add("speaker notes on most slides", len(noted) * 2 >= len(slides), f"{len(noted)}/{len(slides)}")
    problems = f.check(d["id"])["problems"]
    over = [p["message"] for p in problems if p["kind"] in ("overflow", "offSlide")]
    c.add("no text overflows, nothing off the slide", not over, "; ".join(over[:2]))
    if chart:
        c.add("a chart on a slide", any(sh.get("kind") == "chart" for s in slides for sh in s.get("shapes", [])))
    c.run("the slides render", lambda: f.look(page=d["id"], slide=1)["kind"] == "slide")


def score_minutes(f: Folio, c: Checks) -> None:
    page = doc_page(f)
    hs = [t.lower() for _, t in headings(f, page)]
    c.add("a Decisions section", any("decision" in h for h in hs), str(hs))
    c.add("an Actions section", any("action" in h for h in hs), str(hs))
    c.add("actions in a table", has_table(f, page))
    text = text_of(f, page).lower()
    for who in ["priya", "tom", "lena"]:
        c.add(f"{who.title()} owns an action", who in text)
    c.add("the next meeting is there", "21 oct" in text or "2026-10-21" in text or "october 21" in text or "21 october" in text)
    no_errors(f, c)


ERRORS_SETUP = [("sheet.setRange", {"page": "Costs", "at": "A1", "values": [
    ["Item", "Units", "Unit price", "Cost", "Share"],
    ["Paper", 40, 4.5, "=B2*C2", "=D2/$D$7"],
    ["Toner", 6, 62, "=B3*C3", "=D3/$D$7"],
    ["Pens", 120, 0.8, "=B4*C4", "=D4/$D$8"],
    ["Folders", 75, 1.2, "=B5*C5", "=D5/$D$7"],
    ["Labels", 30, 2.1, "=B6*C6", "=D6/$D$7"],
    ["Total", "=SUMM(B2:B6)", "", "=SUM(D2:D5)", ""],
]})]


def score_errors(f: Folio, c: Checks) -> None:
    sheet = sheet_named(f, "Costs")
    cells = f.cells(sheet["id"]) if sheet else {}
    errors = [a for a, cell in cells.items() if isinstance(cell.get("value"), dict) and ("error" in cell["value"] or "Error" in cell["value"])]
    errors += [a for a, cell in cells.items() if str(cell.get("value", "")).startswith("#")]
    c.add("no cell shows an error", not errors, str(errors))
    total = 40 * 4.5 + 6 * 62 + 120 * 0.8 + 75 * 1.2 + 30 * 2.1
    c.add("the cost total covers every row (801)", find_value(cells, total) is not None)
    c.add("the units total is right (271)", find_value(cells, 271) is not None)
    shares = [number(cells.get(f"E{r}", {}).get("value")) for r in range(2, 7)]
    c.add("shares add up to 100%", all(s is not None for s in shares) and close(sum(shares), 1.0, 0.001), str(shares))
    no_errors(f, c, sheet["id"] if sheet else None)


def score_import(f: Folio, c: Checks) -> None:
    sheets = f.pages_of("sheet")
    c.add("the CSV came in as a sheet", bool(sheets), str([p["name"] for p in sheets]))
    if not sheets:
        return
    cells = {}
    for p in sheets:
        cells.update(f.cells(p["id"]))
    c.add("the invoice total is a formula equal to 4,830", find_value(cells, 4830) is not None)
    docs = f.pages_of("doc")
    c.add("the document page is still there", bool(docs))
    no_errors(f, c)


def score_linked(f: Folio, c: Checks) -> None:
    links = f.call("link.list")
    tables = [l for l in links if "table" in l["what"]]
    charts = [l for l in links if "chart" in l["what"] and l["page"] != "Budget"]
    c.add("a live table of the Budget sheet in the document", any("budget" in l["link"].lower() for l in tables), str(links))
    c.add("a live chart in the document", bool(charts), str(charts))
    c.add("every link resolves", links and all(l["ok"] for l in links))
    no_errors(f, c)


BUDGET_SETUP = [("sheet.setRange", {"page": "Budget", "at": "A1", "values": [
    ["Item", "Monthly", "Yearly"],
    ["Rent", 1150, "=B2*12"], ["Food", 420, "=B3*12"], ["Transport", 95, "=B4*12"], ["Phone", 60, "=B5*12"],
    ["Total", "=SUM(B2:B5)", "=SUM(C2:C5)"],
]}), ("page.add", {"kind": "doc", "name": "Report"}), ("doc.write", {"page": "Report", "replace": True, "markdown": "# Household budget\n\nThis note explains where the money goes each month.\n"})]


MINUTES = """Transcript, product sync, 7 Oct 2026. Present: Priya (PM), Tom (eng), Lena (design), Sam (support). Apologies: Ravi.
Priya: first, the export bug. Tom says the PDF export drops footnotes; fix is small. Decision: Tom fixes it for the 0.3 release, by Friday 10 Oct.
Lena showed the new onboarding screens. Everyone liked them except the third screen, too much text. Decision: ship the new onboarding in 0.3 with screen three cut down. Lena to send the final screens by 14 Oct.
Sam: support gets many questions about templates. Priya: let's write a help page. Sam will draft it by 17 Oct, Priya reviews.
Pricing page: no decision, Priya to bring two options next time.
Next meeting 21 Oct, 10:00."""


JOBS: list[Job] = [
    Job("report-from-notes", "Report from notes", f"Turn these notes into a proper report for the board, in the document page.\n\n{NOTES}", score_report, skill="report-from-notes"),
    Job(
        "cover-letter",
        "Cover letter",
        "Write a one-page cover letter from me, Marion Lefèvre (marion.lefevre@example.com, 12 rue Vendôme, 69006 Lyon), to Atelier Nord (hiring team, 4 quai de Bondy, 69005 Lyon) for their Product Designer job. I have 6 years at a furniture start-up where I led the redesign of the online configurator (conversion +23%) and set up the design system. Date it 7 October 2026.",
        score_letter,
        skill="letter-or-cv",
    ),
    Job(
        "budget-sheet",
        "Budget sheet",
        "Make me a monthly household budget sheet: rent 1150, groceries 420, transport 95, phone and internet 60, leisure 180, insurance 75 (euros a month). Show the monthly and the yearly amount for each line and the totals, nicely formatted.",
        score_budget,
        start="sheet",
        skill="budget-model",
    ),
    Job(
        "clean-and-summarise",
        "Clean data and summarise it",
        "The Raw sheet has sales pasted from an export: messy region names, amounts as text, an empty row. Clean it up and make a summary with the total and the number of sales per region, plus a grand total.",
        score_clean,
        start="sheet",
        setup=[("page.rename", {"page": "1", "name": "Raw"}), ("sheet.setRange", {"page": "Raw", "at": "A1", "values": SALES_ROWS})],
        skill="clean-and-summarise",
    ),
    Job("chart-from-data", "Chart from data", "Add a chart of the monthly sales by region on the Sales sheet.", score_chart, start="sheet", setup=[("page.rename", {"page": "1", "name": "Sales"})] + SALES_SETUP, skill="chart-from-data"),
    Job(
        "deck-from-document",
        "Deck from a document",
        "Make a short deck from the report for the leadership meeting.",
        lambda f, c: score_deck(f, c, 5, 10),
        setup=REPORT_SETUP,
        skill="deck-from-document",
    ),
    Job(
        "pitch-deck",
        "Pitch deck",
        "Make a pitch deck for Fournée, an app that lets neighbourhood bakeries take pre-orders for the morning. 140 bakeries in Lyon use it, monthly revenue 9,800 EUR growing 15% a month, we take 4% per order, we are raising 600k EUR to reach Paris and Bordeaux. Founders: Inès (ex-baker) and Karim (ex-Deliveroo). Put the monthly revenue for the last 6 months (4,300; 5,000; 5,900; 6,900; 8,300; 9,800) in a sheet and chart it in the deck.",
        lambda f, c: score_deck(f, c, 8, 14, chart=True),
        start="blank",
        skill="pitch-deck",
    ),
    Job("meeting-minutes", "Meeting minutes", f"Write the minutes of this meeting.\n\n{MINUTES}", score_minutes, skill="meeting-minutes"),
    Job(
        "fix-formula-errors",
        "Fix formula errors",
        "The Costs sheet has errors and the totals look wrong. Fix it.",
        score_errors,
        start="sheet",
        setup=[("page.rename", {"page": "1", "name": "Costs"})] + ERRORS_SETUP,
        skill="fix-formula-errors",
    ),
    Job(
        "import-csv",
        "Import a CSV",
        "Bring the invoice lines from {fixtures}/invoice.csv into this file as a sheet, and add a total of the amounts under them.",
        score_import,
        skill="import-convert",
    ),
    Job(
        "linked-report",
        "Live links between pages",
        "In the Report, add the budget as a table that stays in step with the Budget sheet, and a chart of the monthly amounts by item, also live.",
        score_linked,
        start="sheet",
        setup=[("page.rename", {"page": "1", "name": "Budget"})] + BUDGET_SETUP,
    ),
]

FIXTURES = {
    "invoice.csv": "Line,Description,Hours,Rate,Amount\n1,Discovery workshop,6,90,540\n2,Wireframes,14,90,1260\n3,Visual design,22,90,1980\n4,Prototype,10,105,1050\n",
}
