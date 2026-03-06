#!/usr/bin/env python3
"""
yoyo eval harness — measures LLM answer accuracy with CC tools vs yoyo tools.

Usage:
    ANTHROPIC_API_KEY=sk-... python3 evals/run.py --tasks evals/tasks/ripgrep.json
    ANTHROPIC_API_KEY=sk-... python3 evals/run.py --tasks evals/tasks/ripgrep.json --ids rg-001,rg-002

How it works:
    For each task, two "agent answers" are generated:
      - CC mode:   runs grep/wc against the codebase, asks Claude to answer from raw text
      - yoyo mode: runs yoyo CLI tools, asks Claude to answer from structured JSON

    A judge (Claude Haiku) scores each answer against verified ground truth.

    Results are written to evals/results/<timestamp>.json.
"""

import argparse
import json
import os
import subprocess
import sys
import textwrap
from datetime import datetime
from pathlib import Path

import anthropic

YOYO = Path.home() / ".local/bin/yoyo"
MODEL_AGENT = "claude-haiku-4-5-20251001"
MODEL_JUDGE = "claude-haiku-4-5-20251001"

client = anthropic.Anthropic()


# ── Tool runners ──────────────────────────────────────────────────────────────

def run(cmd: list[str]) -> str:
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=15)
        return r.stdout.strip()
    except Exception as e:
        return f"(error: {e})"


def cc_tool_output(task: dict, codebase_path: str) -> str:
    """Simulate what Claude Code native tools return for a given task type."""
    ttype = task["type"]
    name  = _symbol_name(task)
    path  = codebase_path

    if ttype in ("definition_location", "visibility", "module_path", "fan_out"):
        grep = run(["grep", "-rn", f"fn {name}", path, "--include=*.rs"])
        return f"Grep output for 'fn {name}':\n{grep or '(no results)'}"

    if ttype == "caller_count":
        grep = run(["grep", "-rn", name, path, "--include=*.rs"])
        wc   = run(["bash", "-c", f"grep -rn '{name}' {path} --include='*.rs' | wc -l"])
        return f"Grep for '{name}' ({wc} total hits including definition/comments):\n{grep[:2000]}"

    if ttype in ("complexity_rank", "health_dead_code", "health_god_functions"):
        return "(No native tool available. Would require reading all files and manual analysis.)"

    return "(No tool output available for this task type)"


def yoyo_tool_output(task: dict, codebase_path: str) -> str:
    """Run the appropriate yoyo tool and return its JSON output."""
    ttype = task["type"]
    name  = _symbol_name(task)

    if ttype in ("definition_location", "visibility", "module_path", "fan_out"):
        return run([str(YOYO), "symbol", "--path", codebase_path, "--name", name])

    if ttype == "caller_count":
        return run([str(YOYO), "blast-radius", "--path", codebase_path,
                    "--symbol", name, "--depth", "1"])

    if ttype in ("complexity_rank", "health_dead_code", "health_god_functions"):
        return run([str(YOYO), "health", "--path", codebase_path])

    return "(No yoyo tool available for this task type)"


def _symbol_name(task: dict) -> str:
    """Extract symbol name from task question (heuristic: first backtick-quoted word)."""
    q = task["question"]
    if "`" in q:
        return q.split("`")[1]
    return ""


# ── Agent ─────────────────────────────────────────────────────────────────────

AGENT_SYSTEM = """\
You are a code intelligence assistant. You will be given a question about a codebase
and the raw output from a code search tool. Answer the question as precisely as possible
using only the information in the tool output. If the tool output does not contain enough
information to answer, say "I cannot determine this from the available output" — do not guess.
"""

def agent_answer(question: str, tool_output: str) -> str:
    resp = client.messages.create(
        model=MODEL_AGENT,
        max_tokens=512,
        system=AGENT_SYSTEM,
        messages=[{
            "role": "user",
            "content": f"Tool output:\n{tool_output}\n\nQuestion: {question}"
        }],
    )
    return resp.content[0].text.strip()


# ── Judge ─────────────────────────────────────────────────────────────────────

JUDGE_SYSTEM = """\
You are a code intelligence evaluator. Score an AI agent's answer against verified ground truth.

Be strict:
- Only mark "correct: true" if the answer explicitly and accurately states the fact
- A hedged or uncertain correct answer gets confidence "low"
- "I cannot determine X" = failure_mode "refused" for that dimension
- A wrong specific value = failure_mode "wrong_fact"
- A confident wrong claim = failure_mode "hallucinated"

Return ONLY valid JSON in this exact shape, no prose, no markdown:
{
  "scores": {
    "<dimension>": {
      "correct": true,
      "confidence": "high|medium|low",
      "failure_mode": "correct|wrong_fact|hallucinated|refused",
      "note": "brief reason (max 20 words)"
    }
  },
  "overall": {
    "correct_count": 1,
    "total": 1,
    "summary": "one sentence"
  }
}
"""

def judge(question: str, ground_truth: dict, answer: str) -> dict:
    msg = textwrap.dedent(f"""\
        QUESTION: {question}

        GROUND TRUTH:
        {json.dumps(ground_truth, indent=2)}

        AGENT ANSWER:
        {answer}

        Score the answer.
    """)
    resp = client.messages.create(
        model=MODEL_JUDGE,
        max_tokens=1024,
        system=JUDGE_SYSTEM,
        messages=[{"role": "user", "content": msg}],
    )
    raw = resp.content[0].text.strip()
    if raw.startswith("```"):
        raw = "\n".join(raw.split("\n")[1:]).rstrip("`").strip()
    return json.loads(raw)


# ── Main ──────────────────────────────────────────────────────────────────────

def run_eval(task_file: str, filter_ids: list[str] | None = None) -> dict:
    with open(task_file) as f:
        suite = json.load(f)

    codebase_path = suite["codebase_path"]
    tasks = suite["tasks"]
    if filter_ids:
        tasks = [t for t in tasks if t["id"] in filter_ids]

    results = []
    totals = {"cc": {"correct": 0, "total": 0}, "yoyo": {"correct": 0, "total": 0}}

    for task in tasks:
        tid   = task["id"]
        q     = task["question"]
        gt    = task["ground_truth"]
        ttype = task["type"]

        print(f"\n{'─'*60}")
        print(f"[{tid}] {ttype}")
        print(f"Q: {q[:80]}...")

        cc_out   = cc_tool_output(task, codebase_path)
        yoyo_out = yoyo_tool_output(task, codebase_path)

        cc_ans   = agent_answer(q, cc_out)
        yoyo_ans = agent_answer(q, yoyo_out)

        cc_score   = judge(q, gt, cc_ans)
        yoyo_score = judge(q, gt, yoyo_ans)

        def print_scores(label, score):
            overall = score["overall"]
            print(f"  {label}: {overall['correct_count']}/{overall['total']} — {overall['summary']}")
            for dim, s in score["scores"].items():
                icon = "✓" if s["correct"] else "✗"
                print(f"    {icon} {dim:25s} [{s['failure_mode']:12s}] {s['note']}")

        print_scores("CC  ", cc_score)
        print_scores("yoyo", yoyo_score)

        cc_c   = cc_score["overall"]["correct_count"]
        cc_t   = cc_score["overall"]["total"]
        yy_c   = yoyo_score["overall"]["correct_count"]
        yy_t   = yoyo_score["overall"]["total"]

        totals["cc"]["correct"]   += cc_c
        totals["cc"]["total"]     += cc_t
        totals["yoyo"]["correct"] += yy_c
        totals["yoyo"]["total"]   += yy_t

        results.append({
            "id": tid,
            "type": ttype,
            "cc":   {"answer": cc_ans,   "score": cc_score},
            "yoyo": {"answer": yoyo_ans, "score": yoyo_score},
        })

    print(f"\n{'═'*60}")
    print("FINAL RESULTS")
    print(f"{'═'*60}")
    for mode in ["cc", "yoyo"]:
        c = totals[mode]["correct"]
        t = totals[mode]["total"]
        pct = round(100 * c / t) if t else 0
        label = "Claude Code" if mode == "cc" else "yoyo       "
        print(f"  {label}: {c}/{t} ({pct}%)")

    run_result = {
        "run_id":       datetime.now().strftime("%Y-%m-%d-%H%M%S"),
        "codebase":     suite["codebase"],
        "yoyo_version": _yoyo_version(),
        "model_agent":  MODEL_AGENT,
        "model_judge":  MODEL_JUDGE,
        "tasks_run":    len(tasks),
        "totals":       totals,
        "tasks":        results,
    }
    return run_result


def _yoyo_version() -> str:
    out = run([str(YOYO), "--version"])
    return out.split()[-1] if out else "unknown"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="yoyo eval harness")
    parser.add_argument("--tasks",  required=True, help="Path to tasks JSON file")
    parser.add_argument("--ids",    help="Comma-separated task IDs to run (default: all)")
    parser.add_argument("--output", help="Output file (default: evals/results/<timestamp>.json)")
    args = parser.parse_args()

    filter_ids = args.ids.split(",") if args.ids else None
    result = run_eval(args.tasks, filter_ids)

    out_dir = Path(__file__).parent / "results"
    out_dir.mkdir(exist_ok=True)
    out_path = args.output or str(out_dir / f"{result['run_id']}.json")
    with open(out_path, "w") as f:
        json.dump(result, f, indent=2)

    print(f"\nResults written to: {out_path}")
