#!/usr/bin/env python3
"""folio's evals (lsuite HARNESS.md part 7): scripted office jobs run headless by the built-in
agent with a real model, each scored by automatic checks on the resulting .folio file.

    python3 evals/run.py                         every job, Claude Code's default model
    python3 evals/run.py budget-sheet pitch-deck only these jobs
    python3 evals/run.py --model haiku --provider claude-code
    python3 evals/run.py --provider anthropic    with ANTHROPIC_API_KEY (or any provider id)
    python3 evals/run.py --cli /tmp/bin/folio-cli  binaries copied elsewhere (folio-mcp beside it)
    python3 evals/run.py --list                  the jobs
    python3 evals/run.py --score DIR             score the files of an earlier run again

Each job runs `folio-cli --file <job>.folio agent "<request>"` in a scratch home (its own data,
config and lsuite folders), so the person's settings, files and account are never touched; the
Claude Code provider uses the `claude` already signed in on this computer, so no key is needed.
Results go to evals/results/<run>/ (the .folio files, the agent's answers, the scores) and a
summary line per job is added to evals/RESULTS.md. Run them before each release: a harness change
that lowers the pass rate doesn't ship.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(HERE))

from folio import Checks, Folio, env_for  # noqa: E402
from jobs import FIXTURES, JOBS, Job  # noqa: E402


def build(profile: str) -> Path:
    args = ["cargo", "build", "-p", "folio-cli", "-p", "folio-mcp"] + (["--release"] if profile == "release" else [])
    subprocess.run(args, cwd=ROOT, check=True)
    return ROOT / "target" / profile / "folio-cli"


def setup(job: Job, f: Folio, fixtures: Path) -> None:
    kind = job.start
    f.call("file.new", title=job.title, kind=kind)
    if job.setup:
        f.batch(job.setup)


def run_agent(job: Job, f: Folio, out: Path, args: argparse.Namespace, fixtures: Path) -> dict:
    prompt = job.prompt.replace("{fixtures}", str(fixtures))
    (out / "prompt.txt").write_text(prompt)
    cmd = [str(f.cli), "--file", str(f.path), "agent", "--prompt-file", str(out / "prompt.txt"), "--json", "--quiet", "--provider", args.provider]
    if args.model:
        cmd += ["--model", args.model]
    if args.max_steps:
        cmd += ["--max-steps", str(args.max_steps)]
    started = time.time()
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, env=f.env, timeout=args.timeout)
        stdout, stderr, code = p.stdout, p.stderr, p.returncode
    except subprocess.TimeoutExpired as e:
        stdout, stderr, code = (e.stdout or b"").decode() if isinstance(e.stdout, bytes) else (e.stdout or ""), "timed out", -1
    (out / "agent.stderr.txt").write_text(stderr or "")
    try:
        answer = json.loads(stdout)
    except json.JSONDecodeError:
        answer = {"ok": False, "error": (stderr or stdout or "no answer")[-2000:]}
    answer["exit"] = code
    answer["wall"] = round(time.time() - started, 1)
    (out / "agent.json").write_text(json.dumps(answer, indent=2, ensure_ascii=False))
    return answer


def score(job: Job, f: Folio) -> Checks:
    c = Checks()
    try:
        job.score(f, c)
    except Exception as e:  # the file isn't what the checks expect at all
        c.add("the file can be scored", False, f"{type(e).__name__}: {e}")
    return c


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("jobs", nargs="*", help="job names (default: all)")
    ap.add_argument("--provider", default="claude-code", help="agent provider id (default claude-code)")
    ap.add_argument("--model", default="", help="model id or alias (default: the provider's)")
    ap.add_argument("--max-steps", type=int, default=0)
    ap.add_argument("--timeout", type=int, default=1200, help="seconds per job")
    ap.add_argument("--profile", default="debug", choices=["debug", "release"])
    ap.add_argument("--no-build", action="store_true")
    ap.add_argument("--cli", metavar="PATH", help="this folio-cli (with folio-mcp beside it) instead of target/<profile>/; implies --no-build")
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--score", metavar="DIR", help="score the files of an earlier run again")
    ap.add_argument("--no-record", action="store_true", help="don't add to RESULTS.md")
    args = ap.parse_args()

    if args.list:
        for j in JOBS:
            print(f"{j.name:22} {j.title}")
        return 0
    chosen = [j for j in JOBS if not args.jobs or j.name in args.jobs]
    unknown = set(args.jobs) - {j.name for j in JOBS}
    if unknown:
        print(f"unknown jobs: {', '.join(sorted(unknown))} (--list)", file=sys.stderr)
        return 2

    cli = Path(args.cli).resolve() if args.cli else ROOT / "target" / args.profile / "folio-cli"
    if not args.no_build and not args.cli:
        cli = build(args.profile)
    keep_account = args.provider in ("lsuite", "lsuite-ai")

    if args.score:
        base = Path(args.score).resolve()
    else:
        stamp = dt.datetime.now().strftime("%Y-%m-%d-%H%M%S")
        base = HERE / "results" / stamp
    base.mkdir(parents=True, exist_ok=True)
    fixtures = base / "fixtures"
    fixtures.mkdir(exist_ok=True)
    for name, text in FIXTURES.items():
        (fixtures / name).write_text(text)

    rows = []
    for job in chosen:
        out = base / job.name
        out.mkdir(exist_ok=True)
        env = env_for(out / "home", keep_account)
        f = Folio(cli, out / f"{job.name}.folio", env)
        if args.score:
            answer = json.loads((out / "agent.json").read_text()) if (out / "agent.json").exists() else {}
        else:
            if f.path.exists():
                f.path.unlink()
            print(f"== {job.name}: setting up", flush=True)
            setup(job, f, fixtures)
            print(f"== {job.name}: the agent is working…", flush=True)
            answer = run_agent(job, f, out, args, fixtures)
            print(f"   {'ok' if answer.get('ok') else 'failed'} in {answer.get('wall')} s, {len(answer.get('commands', []))} commands", flush=True)
        checks = score(job, f)
        for r in checks.results:
            print(f"   {'✓' if r.ok else '✗'} {r.name}{f'  ({r.detail})' if r.detail and not r.ok else ''}")
        passed = checks.passed == len(checks.results) and answer.get("ok", False)
        row = {
            "job": job.name,
            "passed": passed,
            "checks": f"{checks.passed}/{len(checks.results)}",
            "score": round(checks.passed / max(1, len(checks.results)), 3),
            "agentOk": answer.get("ok"),
            "error": answer.get("error"),
            "commands": len(answer.get("commands", [])),
            "seconds": answer.get("wall"),
            "looked": sum(1 for c in answer.get("commands", []) if c.get("command") == "harness.look"),
            "checked": sum(1 for c in answer.get("commands", []) if c.get("command") == "harness.check"),
            "skill": any(c.get("command") == "harness.skill" for c in answer.get("commands", [])),
            "results": [r.__dict__ for r in checks.results],
        }
        (out / "score.json").write_text(json.dumps(row, indent=2, ensure_ascii=False))
        rows.append(row)

    model = args.model or "default"
    total = len(rows)
    ok = sum(1 for r in rows if r["passed"])
    mean = sum(r["score"] for r in rows) / max(1, total)
    summary = {"date": dt.date.today().isoformat(), "provider": args.provider, "model": model, "jobs": total, "passed": ok, "passRate": round(ok / max(1, total), 3), "checkScore": round(mean, 3), "rows": rows}
    (base / "summary.json").write_text(json.dumps(summary, indent=2, ensure_ascii=False))
    print(f"\n{ok}/{total} jobs passed, checks {mean:.0%} — {base}")
    if not args.no_record:
        record(summary, base)
    return 0 if ok == total else 1


def record(summary: dict, base: Path) -> None:
    path = HERE / "RESULTS.md"
    if not path.exists():
        path.write_text(HEADER)
    lines = [
        f"\n## {summary['date']} · {summary['provider']} · {summary['model']} · {summary['passed']}/{summary['jobs']} jobs passed · checks {summary['checkScore']:.0%}\n",
        f"\nRun `{base.relative_to(ROOT) if base.is_relative_to(ROOT) else base}`.\n\n",
        "| Job | Result | Checks | Commands | Looked | Checked | Skill | Seconds | Failed checks |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    ]
    for r in summary["rows"]:
        failed = "; ".join(f"{x['name']}" + (f" ({x['detail'][:80]})" if x["detail"] else "") for x in r["results"] if not x["ok"])
        if r["agentOk"] is False and r.get("error"):
            failed = f"agent: {str(r['error'])[:120]}" + (f"; {failed}" if failed else "")
        lines.append(f"| {r['job']} | {'pass' if r['passed'] else 'fail'} | {r['checks']} | {r['commands']} | {r['looked']} | {r['checked']} | {'yes' if r['skill'] else 'no'} | {r['seconds']} | {failed.replace('|', '/') or '—'} |\n")
    with path.open("a") as fh:
        fh.writelines(lines)


HEADER = """# folio eval results

Scripted office jobs (`evals/jobs.py`) run headless by folio's built-in agent with a real model
(`python3 evals/run.py`), each scored by automatic checks on the resulting file. A job passes when
the agent finished and every check passed. "Looked" and "Checked" count the agent's
`harness.look` and `harness.check` calls (the finish routine); "Skill" says whether it loaded one.
Newest runs at the bottom.
"""


if __name__ == "__main__":
    sys.exit(main())
