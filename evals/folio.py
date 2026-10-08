"""Helpers the eval jobs use to set up and score a .folio file through folio-cli.

Everything goes through `folio-cli --file <job.folio> <command>`, the same registry the agent
uses, so a check reads exactly what the agent produced.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable


class CliError(RuntimeError):
    pass


@dataclass
class Folio:
    """One .folio file, driven through folio-cli."""

    cli: Path
    path: Path
    env: dict[str, str]

    def call(self, command: str, **params: Any) -> Any:
        args = [str(self.cli), "--file", str(self.path), command, "--compact"]
        if params:
            args += ["--args", json.dumps(params)]
        p = subprocess.run(args, capture_output=True, text=True, env=self.env, timeout=120)
        if p.returncode != 0:
            raise CliError(f"{command} {params}: {p.stderr.strip() or p.stdout.strip()}")
        out = p.stdout.strip()
        return json.loads(out) if out else None

    def batch(self, lines: list[tuple[str, dict]]) -> None:
        text = "\n".join(json.dumps({"command": c, "params": p}) for c, p in lines) + "\n"
        p = subprocess.run([str(self.cli), "--file", str(self.path), "batch"], input=text, capture_output=True, text=True, env=self.env, timeout=300)
        if p.returncode != 0:
            raise CliError(f"setup batch failed: {p.stderr.strip()}\n{p.stdout[-2000:]}")

    # ---- reading ---------------------------------------------------------------------------

    def pages(self) -> list[dict]:
        return self.call("page.list") or []

    def pages_of(self, kind: str) -> list[dict]:
        return [p for p in self.pages() if p["kind"] == kind]

    def page(self, key: str | int) -> dict:
        return self.call("page.get", page=str(key))

    def cells(self, key: str | int) -> dict[str, dict]:
        """A sheet's cells by A1 address: {input, value} (value as stored: numbers, text…)."""
        return self.page(key).get("cells", {}) or {}

    def check(self, page: str | None = None) -> dict:
        return self.call("harness.check", **({"page": page} if page else {}))

    def doc_blocks(self, key: str | int) -> list[dict]:
        return self.call("doc.read", page=str(key))["blocks"]

    def doc_markdown(self, key: str | int) -> str:
        r = self.call("doc.read", page=str(key), markdown=True)
        return r if isinstance(r, str) else r.get("markdown", "")

    def outline(self, key: str | int) -> list[dict]:
        r = self.call("doc.outline", page=str(key))
        return r if isinstance(r, list) else r.get("outline", r.get("headings", []))

    def deck(self, key: str | int) -> dict:
        return self.call("deck.read", page=str(key))

    def evaluate(self, sheet: str, formula: str) -> Any:
        return self.call("sheet.evaluate", page=sheet, formula=formula)

    def look(self, **params: Any) -> dict:
        return self.call("harness.look", **params)


def number(v: Any) -> float | None:
    """A number from a cell value or shown text ("1,234.50", "12 %", "€ 3")."""
    if isinstance(v, bool) or v is None:
        return None
    if isinstance(v, (int, float)):
        return float(v)
    if isinstance(v, dict):
        for k in ("number", "Number", "value"):
            if k in v:
                return number(v[k])
        return None
    s = str(v).strip().replace(" ", " ").replace(" ", " ")
    pct = s.endswith("%")
    s = re.sub(r"[^0-9.,\-]", "", s)
    if s.count(",") and s.count("."):
        s = s.replace(",", "")
    elif s.count(",") == 1 and len(s.split(",")[1]) != 3:
        s = s.replace(",", ".")
    else:
        s = s.replace(",", "")
    try:
        n = float(s)
    except ValueError:
        return None
    return n / 100 if pct else n


def values(cells: dict[str, dict]) -> list[float]:
    out = []
    for c in cells.values():
        n = number(c.get("value"))
        if n is not None:
            out.append(n)
    return out


def close(a: float | None, b: float, tol: float = 0.01) -> bool:
    return a is not None and abs(a - b) <= max(tol, abs(b) * 1e-6)


@dataclass
class Result:
    name: str
    ok: bool
    detail: str = ""


@dataclass
class Checks:
    """What a job's checks found."""

    results: list[Result] = field(default_factory=list)

    def add(self, name: str, ok: bool, detail: str = "") -> bool:
        self.results.append(Result(name, bool(ok), detail))
        return bool(ok)

    def run(self, name: str, fn: Callable[[], tuple[bool, str] | bool]) -> bool:
        try:
            r = fn()
        except Exception as e:  # a check that can't run fails, with why
            return self.add(name, False, f"{type(e).__name__}: {e}")
        if isinstance(r, tuple):
            return self.add(name, r[0], r[1])
        return self.add(name, r)

    @property
    def passed(self) -> int:
        return sum(1 for r in self.results if r.ok)


def env_for(home: Path, keep_account: bool) -> dict[str, str]:
    env = dict(os.environ)
    env["FOLIO_DATA_DIR"] = str(home / "data")
    env["FOLIO_CONFIG_DIR"] = str(home / "config")
    env["FOLIO_NO_UPDATE"] = "1"
    if not keep_account:
        env["LSUITE_HOME"] = str(home / "lsuite")
    return env
