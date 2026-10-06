#!/usr/bin/env python3
"""Writes folio's mark and app icon: a lowercase f cut square, its arm ending on a folded page
corner, a sharp crossbar, and the page below it dissolving into ordered dither (the grain of the
interface), in one colour. A sibling of kimchi's napa stalk.

    brand/mark.svg                               ink on paper
    brand/icon.svg                               the app icon (then run scripts/make-icons.sh)
    crates/folio-desktop/assets/icons/mark.svg   the window's copy, in currentColor
"""
import os

# On a 64-unit grid, centred like kimchi's mark (x 13-48, y 10-54).
STEM = [(13, 54), (13, 17), (20, 10), (37, 10), (37, 18), (21, 18), (21, 54)]  # f: stem and arm
EAR = [(40, 10), (48, 18), (40, 18)]  # the folded corner at the end of the arm
BAR = [(23.5, 26), (41, 26), (37, 33), (23.5, 33)]  # the crossbar, cut sharp
PAGE = [(23.5, 36), (35.5, 36), (44, 54), (23.5, 54)]  # the page, dissolving toward its corner


def inside(p, poly):
    x, y = p
    c = False
    n = len(poly)
    for i in range(n):
        x1, y1 = poly[i]
        x2, y2 = poly[(i + 1) % n]
        if (y1 > y) != (y2 > y) and x < (x2 - x1) * (y - y1) / (y2 - y1) + x1:
            c = not c
    return c


B = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]]  # 4x4 Bayer matrix


def dots(cell=2.2, size=1.75):
    out = []
    (bx, by), (tx, ty) = PAGE[0], PAGE[2]
    j = 0
    y = 36.0
    while y < 54:
        i = 0
        x = 23.5
        while x < 46:
            c = (x + cell / 2, y + cell / 2)
            if inside(c, PAGE):
                t = ((c[0] - bx) * (tx - bx) + (c[1] - by) * (ty - by)) / ((tx - bx) ** 2 + (ty - by) ** 2)
                level = 1 - 0.75 * max(0, min(1, t)) ** 1.3
                if level > (B[j % 4][i % 4] + 0.5) / 16:
                    out.append(f'<rect x="{x:.2f}" y="{y:.2f}" width="{size}" height="{size}"/>')
            x += cell
            i += 1
        y += cell
        j += 1
    return out


def pts(p):
    return " ".join(f"{x},{y}" for x, y in p)


def mark(fill="currentColor"):
    return (f'<g fill="{fill}">'
            + "".join(f'<polygon points="{pts(p)}"/>' for p in (STEM, EAR, BAR))
            + "".join(dots()) + '</g>')


def icon():
    tile = ("M383.41 100 L640.59 100 C722.19 100 763 100 799.79 112.14 L806.92 113.89 C854.88 131.34 892.66 169.12 910.11 217.08 C924 261 924 301.81 924 383.41 L924 640.59 C924 722.19 924 763 911.86 799.79 L910.11 806.92 C892.66 854.88 854.88 892.66 806.92 910.11 C763 924 722.19 924 640.59 924 L383.41 924 C301.81 924 261 924 224.21 911.86 L217.08 910.11 C169.12 892.66 131.34 854.88 113.89 806.92 C100 763 100 722.19 100 640.59 L100 383.41 C100 301.81 100 261 112.14 224.21 L113.89 217.08 C131.34 169.12 169.12 131.34 217.08 113.89 C261 100 301.81 100 383.41 100 Z")
    # Dithered light in the top-left corner of the tile, as on the app's page.
    corner = []
    cell = 16
    for j in range(0, 26):
        for i in range(0, 26):
            x = 100 + i * cell
            y = 100 + j * cell
            d = ((i / 26) ** 2 + (j / 26) ** 2) ** 0.5 / 1.2
            level = max(0, 1 - d * 1.5) ** 1.4
            if level > (B[j % 4][i % 4] + 0.5) / 16:
                corner.append(f'<rect x="{x}" y="{y}" width="{cell - 5}" height="{cell - 5}"/>')
    return (f'''<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
  <!-- folio app icon: the macOS icon grid (824 px continuous-corner tile on 1024) in near black,
       a corner of dithered light like the app's page, and the mark (brand/mark.svg) in white at
       about 58 % of the tile. scripts/gen-mark.py writes this file; scripts/make-icons.sh renders
       every size from it. -->
  <defs>
    <path id="tile" d="{tile}"/>
    <clipPath id="tile-clip"><use href="#tile"/></clipPath>
  </defs>
  <use href="#tile" fill="#0b0b0b"/>
  <g clip-path="url(#tile-clip)" fill="#fff" fill-opacity="0.16">{"".join(corner)}</g>
  <use href="#tile" fill="none" stroke="#fff" stroke-opacity="0.16" stroke-width="3"/>
  <g transform="translate(512 512) scale(10.86) translate(-30.5 -32)">{mark("#fff")}</g>
</svg>''')


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(text + "\n")


if __name__ == "__main__":
    os.chdir(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
    write("crates/folio-desktop/assets/icons/mark.svg",
          '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">' + mark() + '</svg>')
    write("brand/mark.svg",
          '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">\n'
          "  <!-- folio's mark: a lowercase f cut square, its arm ending on a folded page corner, a\n"
          "       sharp crossbar and the page dissolving into dither (the grain of the interface).\n"
          "       One colour: ink on paper or paper on ink. scripts/gen-mark.py writes it. -->\n  "
          + mark("#0a0a0a") + "\n</svg>")
    write("brand/icon.svg", icon())
