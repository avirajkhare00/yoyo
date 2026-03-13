#!/usr/bin/env python3
"""
Run puncture compare evals against the latest N yoyo tags.

Default flow:
- select latest 5 semver tags
- build a yoyo binary for each tag into /tmp
- run the standard compare tasks for each tag
- write per-tag reports under evals/results/by-tag/<tag>/
- write one summary JSON across all requested tags/tasks
"""

from __future__ import annotations

import argparse
import json
import os
import shlex
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


DEFAULT_TASKS = ["uuid", "httprouter", "semver"]


def run(cmd: list[str], cwd: Path, env: dict[str, str] | None = None, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, cwd=cwd, env=env, check=check, text=True, capture_output=True)


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def harness_dir() -> Path:
    return Path(__file__).resolve().parent


def latest_tags(root: Path, count: int) -> list[str]:
    proc = run(["git", "tag", "--sort=-version:refname"], cwd=root)
    tags = [line.strip() for line in proc.stdout.splitlines() if line.strip().startswith("v")]
    return tags[:count]


def build_tag_binary(root: Path, tag: str, cache_root: Path, dry_run: bool) -> Path:
    tag_root = cache_root / tag
    src_dir = tag_root / "src"
    target_dir = tag_root / "target"
    binary = target_dir / "release" / "yoyo"

    if binary.exists():
        return binary

    if dry_run:
        return binary

    if src_dir.exists():
        subprocess.run(["rm", "-rf", str(src_dir)], check=True)
    src_dir.mkdir(parents=True, exist_ok=True)

    archive_cmd = f"git archive {shlex.quote(tag)} | tar -x -C {shlex.quote(str(src_dir))}"
    subprocess.run(["/bin/sh", "-lc", archive_cmd], cwd=root, check=True)

    env = dict(os.environ)
    env["CARGO_TARGET_DIR"] = str(target_dir)
    subprocess.run(["cargo", "build", "--release", "--locked"], cwd=src_dir, env=env, check=True)

    if not binary.exists():
        raise RuntimeError(f"expected built binary at {binary}")
    return binary


def task_dir(root: Path, task_name: str) -> Path:
    return root / "evals" / "tasks" / task_name


def task_id_for(task_name: str) -> str:
    mapping = {
        "uuid": "uuid-go-001",
        "httprouter": "httprouter-go-001",
        "semver": "semver-rust-001",
    }
    try:
        return mapping[task_name]
    except KeyError as exc:
        raise RuntimeError(f"unknown task id mapping for {task_name}") from exc


def latest_report(results_dir: Path, task_name: str) -> Path | None:
    task_id = task_id_for(task_name)
    matches = sorted(results_dir.glob(f"*-{task_id}-compare.json"))
    return matches[-1] if matches else None


def build_treatment_command(binary: Path) -> str:
    return (
        'python3 "$YOYO_EVAL_HARNESS_DIR/codex_runner.py" '
        f'--mode treatment --yoyo-command {shlex.quote(str(binary))}'
    )


def run_compare_for_tag(
    *,
    root: Path,
    tag: str,
    binary: Path,
    tasks: list[str],
    timeout: str,
    dry_run: bool,
) -> list[dict[str, Any]]:
    results_dir = root / "evals" / "results" / "by-tag" / tag
    results_dir.mkdir(parents=True, exist_ok=True)
    harness = harness_dir()
    summaries: list[dict[str, Any]] = []

    for task_name in tasks:
        compare_cmd = [
            "go",
            "run",
            "main.go",
            "--compare",
            "--task",
            str(task_dir(root, task_name)),
            "--control-cmd",
            'python3 "$YOYO_EVAL_HARNESS_DIR/codex_runner.py" --mode control',
            "--treatment-cmd",
            build_treatment_command(binary),
            "--results",
            str(results_dir),
            "--timeout",
            timeout,
        ]

        if dry_run:
            summaries.append(
                {
                    "tag": tag,
                    "task": task_name,
                    "binary": str(binary),
                    "command": compare_cmd,
                    "status": "dry_run",
                }
            )
            continue

        before = set(results_dir.glob("*.json"))
        proc = subprocess.run(compare_cmd, cwd=harness, text=True, capture_output=True)
        after = set(results_dir.glob("*.json"))

        report = latest_report(results_dir, task_name)
        if report is None:
            new_reports = sorted(after - before)
            report = new_reports[-1] if new_reports else None

        summary: dict[str, Any] = {
            "tag": tag,
            "task": task_name,
            "binary": str(binary),
            "returncode": proc.returncode,
            "stdout_tail": proc.stdout[-1200:],
            "stderr_tail": proc.stderr[-1200:],
        }

        if report and report.exists():
            data = json.loads(report.read_text())
            summary["report"] = str(report)
            summary["winner"] = data["summary"]["winner"]
            summary["reason"] = data["summary"]["reason"]
            summary["control"] = {
                "pass": data["control"]["pass"],
                "elapsed_ms": data["control"]["runner"]["elapsed_ms"],
                "tool_calls": data["control"]["metrics"].get("tool_calls"),
            }
            summary["treatment"] = {
                "pass": data["treatment"]["pass"],
                "elapsed_ms": data["treatment"]["runner"]["elapsed_ms"],
                "tool_calls": data["treatment"]["metrics"].get("tool_calls"),
            }
        else:
            summary["report"] = None

        summaries.append(summary)

    return summaries


def summarize_runs(runs: list[dict[str, Any]]) -> dict[str, Any]:
    completed = [run for run in runs if run.get("status") != "dry_run"]
    wins = {"control": 0, "treatment": 0, "tie": 0}
    for run in completed:
        winner = run.get("winner")
        if winner in wins:
            wins[winner] += 1

    return {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "total_runs": len(runs),
        "completed_runs": len(completed),
        "wins": wins,
        "runs": runs,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Run puncture compare evals against the latest yoyo tags")
    parser.add_argument("--count", type=int, default=5, help="number of latest tags to evaluate")
    parser.add_argument("--tags", nargs="*", help="explicit tag list; overrides --count")
    parser.add_argument("--tasks", nargs="*", default=DEFAULT_TASKS, help="task directories under evals/tasks")
    parser.add_argument("--timeout", default="30m", help="compare timeout passed through to the harness")
    parser.add_argument("--cache-root", default="/tmp/yoyo-tag-evals", help="where built tag binaries are cached")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    root = repo_root()
    tags = args.tags or latest_tags(root, args.count)
    cache_root = Path(args.cache_root)
    cache_root.mkdir(parents=True, exist_ok=True)

    runs: list[dict[str, Any]] = []
    for tag in tags:
        binary = build_tag_binary(root, tag, cache_root, args.dry_run)
        runs.extend(
            run_compare_for_tag(
                root=root,
                tag=tag,
                binary=binary,
                tasks=args.tasks,
                timeout=args.timeout,
                dry_run=args.dry_run,
            )
        )

    summary = summarize_runs(runs)
    out_dir = root / "evals" / "results" / "by-tag"
    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = out_dir / f"summary-latest-tags-{datetime.now(timezone.utc).strftime('%Y-%m-%d-%H%M%S')}.json"
    out_path.write_text(json.dumps(summary, indent=2) + "\n")
    print(out_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
