#!/usr/bin/env python3
"""
yoyo semantic search eval — scores embedding-backed search top-N accuracy.

Usage:
    python3 evals/run_semantic.py --tasks evals/tasks/tokio_semantic.json

Scoring: a task passes if ANY expected function name appears in the top-3 results.
"""

import argparse
import json
import subprocess
from datetime import datetime
from pathlib import Path

YOYO = Path.home() / ".local/bin/yoyo"
TOP_N = 3


def run_semantic(codebase_path: str, query: str, limit: int = 5) -> list[dict]:
    result = subprocess.run(
        [str(YOYO), "semantic-search", "--path", codebase_path, "--query", query, "--limit", str(limit)],
        capture_output=True, text=True, timeout=30,
    )
    if result.returncode != 0:
        return []
    try:
        d = json.loads(result.stdout)
        return d.get("results", [])
    except Exception:
        return []


def score_task(task: dict, codebase_path: str) -> dict:
    query = task["query"]
    expected = [e.lower() for e in task["ground_truth"]["expected_in_top3"]]
    results = run_semantic(codebase_path, query, limit=TOP_N)

    top_names = [r["name"].lower() for r in results[:TOP_N]]
    hit = any(exp in top_names for exp in expected)

    return {
        "id": task["id"],
        "query": query,
        "expected": task["ground_truth"]["expected_in_top3"],
        "top_results": [{"name": r["name"], "file": r["file"].split("/")[-1], "score": round(r["score"], 3)} for r in results[:TOP_N]],
        "pass": hit,
        "backend": "embeddings" if results and results[0]["score"] < 2.0 else "tfidf",
    }


def run_eval(tasks_path: str) -> None:
    tasks_file = Path(tasks_path)
    spec = json.loads(tasks_file.read_text())
    codebase_path = spec["codebase_path"]

    results = []
    passed = 0

    sep = "─" * 60
    print(sep)

    for task in spec["tasks"]:
        r = score_task(task, codebase_path)
        results.append(r)

        status = "PASS ✓" if r["pass"] else "FAIL ✗"
        print(f"[{r['id']}] {status}  \"{r['query']}\"")
        print(f"  expected any of: {r['expected']}")
        for i, m in enumerate(r["top_results"], 1):
            marker = " ← HIT" if m["name"].lower() in [e.lower() for e in r["expected"]] else ""
            print(f"  #{i}: {m['name']} ({m['file']}) score={m['score']}{marker}")
        print(f"  backend: {r['backend']}")
        print(sep)

        if r["pass"]:
            passed += 1

    total = len(results)
    print(f"\nSCORE: {passed}/{total} ({100*passed//total}%)  — top-{TOP_N} accuracy")

    # Write results
    out_dir = Path("evals/results")
    out_dir.mkdir(parents=True, exist_ok=True)
    ts = datetime.now().strftime("%Y-%m-%d-%H%M%S")
    out_path = out_dir / f"semantic-{ts}.json"
    out_path.write_text(json.dumps({
        "eval": "semantic_search",
        "codebase": spec["codebase"],
        "score": f"{passed}/{total}",
        "pct": 100 * passed // total,
        "tasks": results,
    }, indent=2))
    print(f"Results written to: {out_path}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="yoyo semantic search eval")
    parser.add_argument("--tasks", required=True, help="Path to tasks JSON file")
    args = parser.parse_args()
    run_eval(args.tasks)
