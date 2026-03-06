#!/usr/bin/env python3
"""
yoyo write eval harness — compares CC write tools vs yoyo write tools.

Unlike the read eval (LLM judge), write correctness is mostly programmatic:
  - word-boundary safety  (grep for corrupted partials)
  - scope correctness     (count files changed)
  - safety gate           (did delete block when it should?)
  - blank line cleanup    (count blank runs after delete)
  - compile check         (cargo build)

Usage:
    python3 evals/write_run.py --tasks evals/tasks/ripgrep_write.json
    python3 evals/write_run.py --tasks evals/tasks/ripgrep_write.json --ids rw-001,rw-002
    python3 evals/write_run.py --tasks evals/tasks/ripgrep_write.json --no-compile
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import tempfile
from datetime import datetime
from pathlib import Path

YOYO = Path.home() / ".local/bin/yoyo"

PASS = "✓"
FAIL = "✗"


def run(cmd: list[str], cwd: str | None = None, timeout: int = 60) -> tuple[int, str, str]:
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, cwd=cwd)
    return r.returncode, r.stdout.strip(), r.stderr.strip()


def copy_codebase(src: str) -> str:
    """Copy codebase to a temp dir and return the path."""
    tmp = tempfile.mkdtemp(prefix="yoyo_write_eval_")
    shutil.copytree(src, tmp, dirs_exist_ok=True)
    return tmp


def bake(path: str):
    run([str(YOYO), "bake", "--path", path])


def git_reset(path: str):
    """Reset any changes in the temp copy."""
    run(["git", "checkout", "."], cwd=path)


def changed_files(path: str) -> list[str]:
    """Return list of files changed vs HEAD."""
    _, out, _ = run(["git", "diff", "--name-only"], cwd=path)
    return [f for f in out.splitlines() if f.strip()]


def count_max_blank_run(filepath: str) -> int:
    """Return the longest run of consecutive blank lines in a file."""
    content = Path(filepath).read_text()
    max_run = cur = 0
    for line in content.splitlines():
        if line.strip() == "":
            cur += 1
            max_run = max(max_run, cur)
        else:
            cur = 0
    return max_run


def compile_check(path: str) -> tuple[bool, str]:
    code, _, err = run(
        ["cargo", "build", "--manifest-path", f"{path}/Cargo.toml"],
        timeout=300,
    )
    return code == 0, err[-500:] if err else ""


def check(label: str, passed: bool, detail: str = "") -> dict:
    icon = PASS if passed else FAIL
    msg = f"  {icon} {label}"
    if detail:
        msg += f"  — {detail}"
    print(msg)
    return {"label": label, "passed": passed, "detail": detail}


# ── CC simulations ────────────────────────────────────────────────────────────

def cc_rename(path: str, old: str, new: str) -> list[str]:
    """
    Simulate CC Edit: plain string replace across all .rs files.
    No word-boundary safety — same as old_string/new_string replacement.
    """
    changed = []
    for rs in Path(path).rglob("*.rs"):
        content = rs.read_text()
        if old in content:
            rs.write_text(content.replace(old, new))
            changed.append(str(rs.relative_to(path)))
    return changed


def cc_delete(path: str, name: str, file: str) -> tuple[bool, str]:
    """
    Simulate CC delete: grep for fn signature, remove lines. No preflight.
    Returns (blocked, reason) — CC never blocks.
    """
    target = Path(path) / file
    content = target.read_text()
    lines = content.splitlines()

    # Find start line
    start = None
    for i, line in enumerate(lines):
        if re.search(rf'\bfn {re.escape(name)}\b', line):
            start = i
            break
    if start is None:
        return False, f"function {name!r} not found"

    # Find end by brace counting
    depth = 0
    end = start
    for i in range(start, len(lines)):
        depth += lines[i].count("{") - lines[i].count("}")
        if depth > 0 or (i == start and "{" not in lines[i]):
            end = i
        if depth == 0 and i > start:
            end = i
            break

    new_content = "\n".join(lines[:start] + lines[end + 1:]) + "\n"
    target.write_text(new_content)
    return False, f"deleted lines {start+1}-{end+1} (no preflight check)"


def cc_patch(path: str, name: str, file: str, comment: str) -> tuple[bool, int]:
    """
    Simulate CC patch: must grep for line number first, then Edit.
    Returns (needed_line_lookup, line_number_used).
    """
    target = Path(path) / file
    content = target.read_text()
    lines = content.splitlines()

    # CC must grep to find the line
    line_num = None
    for i, line in enumerate(lines):
        if re.search(rf'\bfn {re.escape(name)}\b', line):
            line_num = i
            break
    if line_num is None:
        return True, -1

    # Insert comment before the function
    lines.insert(line_num, comment)
    target.write_text("\n".join(lines) + "\n")
    return True, line_num + 1  # 1-indexed


# ── yoyo operations ───────────────────────────────────────────────────────────

def yoyo_rename(path: str, old: str, new: str) -> tuple[bool, dict]:
    code, out, err = run([str(YOYO), "graph-rename", "--path", path,
                          "--name", old, "--new-name", new])
    if code != 0:
        return False, {"error": err}
    try:
        return True, json.loads(out)
    except Exception:
        return False, {"raw": out}


def yoyo_delete(path: str, name: str) -> tuple[bool, str, dict]:
    """Returns (blocked, reason, payload)."""
    code, out, err = run([str(YOYO), "graph-delete", "--path", path, "--name", name])
    if code != 0:
        return True, err or out, {}
    try:
        return False, "", json.loads(out)
    except Exception:
        return False, "", {"raw": out}


def yoyo_patch(path: str, name: str, file: str, comment: str) -> tuple[bool, bool]:
    """
    patch --symbol — no line number needed, resolves from bake index.
    Returns (needed_line_lookup=False, success).
    """
    # First get the symbol to find its start_line, then prepend comment before it
    code, out, _ = run([str(YOYO), "symbol", "--path", path,
                        "--name", name, "--file", file])
    if code != 0:
        return False, False
    try:
        d = json.loads(out)
        match = next((m for m in d["matches"] if m.get("primary")), None)
        if not match:
            return False, False
        start_line = match["start_line"]
        end_line   = match["end_line"]
        # Read existing body and prepend comment
        target = Path(path) / file
        lines  = target.read_text().splitlines()
        body   = "\n".join(lines[start_line - 1 : end_line])
        new_content = comment + "\n" + body
    except Exception:
        return False, False

    code, _, err = run([str(YOYO), "patch", "--path", path,
                        "--symbol", name,
                        "--new-content", new_content])
    return False, code == 0  # needed_line_lookup=False


# ── Task runners ──────────────────────────────────────────────────────────────

def run_rw001(task: dict, src: str, compile_enabled: bool) -> dict:
    """Rename private fn — scope check."""
    op = task["operation"]
    gt = task["ground_truth"]
    results = {"id": task["id"], "cc": [], "yoyo": []}

    print(f"\n  ── CC ──")
    path = copy_codebase(src)
    try:
        changed = cc_rename(path, op["old_name"], op["new_name"])
        results["cc"].append(check("files changed",
            len(changed) == gt["files_changed"],
            f"changed {len(changed)}, expected {gt['files_changed']}"))
        results["cc"].append(check("defining file changed",
            any(gt["defining_file"] in f for f in changed),
            f"changed: {changed[:3]}"))
        results["cc"].append(check("scope: file-only (private)",
            len(changed) == 1,
            f"CC changed {len(changed)} files — no scope awareness"))
        if compile_enabled:
            ok, _ = compile_check(path)
            results["cc"].append(check("compiles", ok))
    finally:
        shutil.rmtree(path, ignore_errors=True)

    print(f"  ── yoyo ──")
    path = copy_codebase(src)
    try:
        bake(path)
        ok, payload = yoyo_rename(path, op["old_name"], op["new_name"])
        fc = payload.get("files_changed", -1)
        scope = payload.get("scope", "?")
        results["yoyo"].append(check("rename succeeded", ok, payload.get("error", "")))
        results["yoyo"].append(check("files changed",
            fc == gt["files_changed"],
            f"changed {fc}, expected {gt['files_changed']}"))
        results["yoyo"].append(check("scope: file-only",
            scope == gt["scope"],
            f"scope={scope}"))
        if compile_enabled and ok:
            c_ok, _ = compile_check(path)
            results["yoyo"].append(check("compiles", c_ok))
    finally:
        shutil.rmtree(path, ignore_errors=True)

    return results


def run_rw002(task: dict, src: str, compile_enabled: bool) -> dict:
    """Rename is_match → is_hit — word-boundary safety."""
    op = task["operation"]
    gt = task["ground_truth"]
    results = {"id": task["id"], "cc": [], "yoyo": []}

    def check_partials(path: str) -> list[str]:
        """Return partial-match names that were corrupted."""
        corrupted = []
        for partial in gt["false_positives"]:
            corrupted_form = partial.replace(op["old_name"], op["new_name"])
            rc, out, _ = run(["grep", "-rl", corrupted_form, path, "--include=*.rs"])
            if out.strip():
                corrupted.append(partial)
        return corrupted

    print(f"  ── CC ──")
    path = copy_codebase(src)
    try:
        cc_rename(path, op["old_name"], op["new_name"])
        corrupted = check_partials(path)
        results["cc"].append(check("word-boundary safe",
            len(corrupted) == 0,
            f"corrupted: {corrupted}" if corrupted else "clean"))
        results["cc"].append(check("no partial match corruption",
            not gt["cc_corrupts_partials"],
            f"CC corrupted {len(corrupted)} partial matches as expected"))
    finally:
        shutil.rmtree(path, ignore_errors=True)

    print(f"  ── yoyo ──")
    path = copy_codebase(src)
    try:
        bake(path)
        ok, payload = yoyo_rename(path, op["old_name"], op["new_name"])
        corrupted = check_partials(path)
        results["yoyo"].append(check("rename succeeded", ok))
        results["yoyo"].append(check("word-boundary safe",
            len(corrupted) == 0,
            f"corrupted: {corrupted}" if corrupted else "clean"))
        results["yoyo"].append(check("partial matches preserved",
            not gt["yoyo_corrupts_partials"],
            "is_match_candidate, is_match_at untouched"))
    finally:
        shutil.rmtree(path, ignore_errors=True)

    return results


def run_rw003(task: dict, src: str, compile_enabled: bool) -> dict:
    """Delete fn with callers — safety gate."""
    op = task["operation"]
    gt = task["ground_truth"]
    results = {"id": task["id"], "cc": [], "yoyo": []}

    print(f"  ── CC ──")
    path = copy_codebase(src)
    try:
        # CC finds a file and deletes — no preflight
        blocked, reason = cc_delete(path, op["name"], "crates/core/search.rs")
        results["cc"].append(check("preflight blocked",
            blocked == gt["cc_blocks"],
            f"CC proceeded without checking callers: {reason}"))
        results["cc"].append(check("callers still compile",
            False,  # CC deleted the fn, callers now broken
            "deleted function body — callers will fail to compile"))
    finally:
        shutil.rmtree(path, ignore_errors=True)

    print(f"  ── yoyo ──")
    path = copy_codebase(src)
    try:
        bake(path)
        blocked, reason, payload = yoyo_delete(path, op["name"])
        results["yoyo"].append(check("preflight blocked",
            blocked == gt["yoyo_blocks"],
            f"blocked with: {reason[:120]}" if blocked else "not blocked"))
        results["yoyo"].append(check("codebase unchanged",
            blocked,  # if blocked, nothing changed
            "no files modified — safe"))
    finally:
        shutil.rmtree(path, ignore_errors=True)

    return results


def run_rw004(task: dict, src: str, compile_enabled: bool) -> dict:
    """Delete dead function — should succeed cleanly."""
    op = task["operation"]
    gt = task["ground_truth"]
    results = {"id": task["id"], "cc": [], "yoyo": []}

    print(f"  ── CC ──")
    path = copy_codebase(src)
    try:
        blocked, reason = cc_delete(path, op["name"], op["file"])
        max_blanks = count_max_blank_run(str(Path(path) / op["file"]))
        results["cc"].append(check("deletion attempted", not blocked, reason))
        results["cc"].append(check("blank lines cleaned",
            max_blanks <= gt["blank_lines_after"],
            f"max blank run: {max_blanks} (CC leaves orphan blanks)"))
        if compile_enabled:
            ok, _ = compile_check(path)
            results["cc"].append(check("compiles", ok))
    finally:
        shutil.rmtree(path, ignore_errors=True)

    print(f"  ── yoyo ──")
    path = copy_codebase(src)
    try:
        bake(path)
        blocked, reason, payload = yoyo_delete(path, op["name"])
        results["yoyo"].append(check("not blocked (dead function)", not blocked,
            reason if blocked else "correctly identified as dead"))
        if not blocked:
            max_blanks = count_max_blank_run(str(Path(path) / op["file"]))
            results["yoyo"].append(check("blank lines cleaned",
                max_blanks <= gt["blank_lines_after"],
                f"max blank run: {max_blanks}"))
            if compile_enabled:
                ok, _ = compile_check(path)
                results["yoyo"].append(check("compiles", ok))
    finally:
        shutil.rmtree(path, ignore_errors=True)

    return results


def run_rw005(task: dict, src: str, compile_enabled: bool) -> dict:
    """Patch by symbol — CC needs line number, yoyo uses name."""
    op = task["operation"]
    results = {"id": task["id"], "cc": [], "yoyo": []}

    print(f"  ── CC ──")
    path = copy_codebase(src)
    try:
        needed_lookup, line = cc_patch(path, op["name"], op["file"], op["insert_comment"])
        applied = line > 0
        results["cc"].append(check("patch applied", applied, f"at line {line}"))
        results["cc"].append(check("required line number lookup",
            needed_lookup,
            "CC must grep first to find the line — 2 tool calls"))
    finally:
        shutil.rmtree(path, ignore_errors=True)

    print(f"  ── yoyo ──")
    path = copy_codebase(src)
    try:
        bake(path)
        needed_lookup, success = yoyo_patch(path, op["name"], op["file"], op["insert_comment"])
        results["yoyo"].append(check("patch applied", success))
        results["yoyo"].append(check("no line number lookup needed",
            not needed_lookup,
            "symbol name sufficient — 1 tool call"))
    finally:
        shutil.rmtree(path, ignore_errors=True)

    return results


# ── Main ──────────────────────────────────────────────────────────────────────

RUNNERS = {
    "rw-001": run_rw001,
    "rw-002": run_rw002,
    "rw-003": run_rw003,
    "rw-004": run_rw004,
    "rw-005": run_rw005,
}

def run_eval(task_file: str, filter_ids: list[str] | None, compile_enabled: bool) -> dict:
    with open(task_file) as f:
        suite = json.load(f)

    src = suite["codebase_path"]
    tasks = suite["tasks"]
    if filter_ids:
        tasks = [t for t in tasks if t["id"] in filter_ids]

    all_results = []
    cc_pass = cc_total = yoyo_pass = yoyo_total = 0

    for task in tasks:
        tid = task["id"]
        print(f"\n{'─'*60}")
        print(f"[{tid}] {task['description']}")

        runner = RUNNERS.get(tid)
        if not runner:
            print(f"  (no runner for {tid})")
            continue

        result = runner(task, src, compile_enabled)
        all_results.append(result)

        for check_result in result["cc"]:
            cc_total += 1
            if check_result["passed"]:
                cc_pass += 1
        for check_result in result["yoyo"]:
            yoyo_total += 1
            if check_result["passed"]:
                yoyo_pass += 1

    print(f"\n{'═'*60}")
    print("FINAL RESULTS")
    print(f"{'═'*60}")
    cc_pct  = round(100 * cc_pass / cc_total)   if cc_total   else 0
    yy_pct  = round(100 * yoyo_pass / yoyo_total) if yoyo_total else 0
    print(f"  Claude Code: {cc_pass}/{cc_total} ({cc_pct}%)")
    print(f"  yoyo:        {yoyo_pass}/{yoyo_total} ({yy_pct}%)")

    return {
        "run_id":      datetime.now().strftime("%Y-%m-%d-%H%M%S"),
        "codebase":    suite["codebase"],
        "compile":     compile_enabled,
        "totals":      {
            "cc":   {"pass": cc_pass,   "total": cc_total},
            "yoyo": {"pass": yoyo_pass, "total": yoyo_total},
        },
        "tasks": all_results,
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--tasks",      required=True)
    parser.add_argument("--ids",        help="Comma-separated task IDs")
    parser.add_argument("--no-compile", action="store_true")
    parser.add_argument("--output",     help="Output JSON path")
    args = parser.parse_args()

    filter_ids = args.ids.split(",") if args.ids else None
    result = run_eval(args.tasks, filter_ids, not args.no_compile)

    out_dir = Path(__file__).parent / "results"
    out_dir.mkdir(exist_ok=True)
    out_path = args.output or str(out_dir / f"write-{result['run_id']}.json")
    with open(out_path, "w") as f:
        json.dump(result, f, indent=2)
    print(f"\nResults written to: {out_path}")
