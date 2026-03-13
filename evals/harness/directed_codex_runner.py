#!/usr/bin/env python3
"""
Directed Codex runner for engineer-command evals.

This wrapper:
- prepares an isolated CODEX_HOME for control or treatment mode
- runs a sequence of engineer commands in one Codex session
- resumes the same session between commands
- writes structured metrics JSON to YOYO_EVAL_METRICS_FILE
"""

from __future__ import annotations

import argparse
import json
import shlex
import tempfile
from pathlib import Path
from typing import Any

from codex_runner import load_task, parse_codex_jsonl, prepare_codex_home, run_codex


def command_is_read_only(command: str) -> bool:
    lowered = command.lower()
    return (
        "do not edit" in lowered
        or "don't edit" in lowered
        or "read-only" in lowered
        or "do not change" in lowered
    )


def load_commands(path: Path) -> list[dict[str, Any]]:
    data = json.loads(path.read_text())
    if not isinstance(data, list):
        raise ValueError("commands file must contain a JSON array")
    commands: list[dict[str, Any]] = []
    for idx, item in enumerate(data, start=1):
        if isinstance(item, str):
            commands.append({"id": f"step-{idx}", "command": item})
            continue
        if isinstance(item, dict) and isinstance(item.get("command"), str):
            commands.append(
                {
                    "id": item.get("id") or f"step-{idx}",
                    "command": item["command"],
                }
            )
            continue
        raise ValueError(f"invalid command entry at index {idx - 1}")
    return commands


def build_step_prompt(
    task: dict[str, Any],
    mode: str,
    command: str,
    *,
    step_index: int,
    total_steps: int,
    prior_result: str | None,
) -> str:
    read_only = command_is_read_only(command)
    lines = [
        "You are working inside a directed tool-use eval on a real repository fixture.",
        "This run evaluates how well you follow a bounded engineer command on the current codebase.",
        "",
        f"Task ID: {task['id']}",
        f"Task name: {task['name']}",
        f"Language: {task['language']}",
    ]
    hints = task.get("hints") or []
    if hints:
        lines.extend(["Hints:"])
        lines.extend(f"- {hint}" for hint in hints)
    if mode == "control":
        lines.extend(
            [
                "",
                "Use Codex built-in tools only. No MCP servers are configured for this run.",
            ]
        )
    else:
        lines.extend(
            [
                "",
                "yoyo MCP is configured for this run.",
                "Before any repo exploration, call boot and index in parallel.",
                "Prefer yoyo search/inspect/impact/change over built-in repo exploration and edit tools when they fit.",
            ]
        )
    lines.extend(
        [
            "",
            "This is a directed tool-use eval.",
            f"Step {step_index} of {total_steps}.",
            "Follow the current engineer command exactly.",
            "Do not ask the evaluator what to do next; make the best reasonable assumption and continue.",
            "Do not inspect git history or hidden oracle material.",
            "Do not run git log, git show, git blame, git diff against other commits, or search for the upstream fix commit/PR.",
            "Do not continue exploring after you have enough evidence to answer this step.",
        ]
    )
    if read_only:
        lines.extend(
            [
                "This step is read-only.",
                "Do not edit files, apply patches, or try to repair the repo in this step.",
                "Do not try to make the task test command pass in this step.",
                "Budget: use at most 8 tool calls for this step, including boot/index.",
                "If the previous engineer-command result already gives enough evidence, answer directly instead of reopening the whole area.",
                "If you run commands, keep them narrowly tied to answering the current question.",
                "Prefer one or two confirming reads over a broad repo sweep.",
                "Do not end with a plan for more work; end with the answer itself.",
                "When you have the answer, stop immediately and report only the result for this step.",
            ]
        )
    else:
        lines.extend(
            [
                "This step may include edits if the engineer command asks for them.",
                f"Task test command: {' '.join(task['test_cmd'])}",
                "If you edit code, keep the patch minimal and verify with the narrowest relevant command before stopping.",
                "After completing the command, stop and report only the result for this step.",
            ]
        )
    lines.extend(
        [
        "",
        ]
    )
    if prior_result:
        lines.extend(
            [
                "",
                "Previous engineer-command result:",
                prior_result.strip(),
            ]
        )
    lines.extend(
        [
            "",
            f"Engineer command: {command}",
        ]
    )
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description="Directed Codex runner for engineer-command evals")
    parser.add_argument("--mode", required=True, choices=["control", "treatment"])
    parser.add_argument("--workspace", required=True)
    parser.add_argument("--task-file", required=True)
    parser.add_argument("--metrics-file", required=True)
    parser.add_argument("--commands-file", required=True)
    parser.add_argument("--model")
    parser.add_argument("--reasoning-effort")
    parser.add_argument("--codex-bin", default="codex")
    parser.add_argument("--base-codex-home", default=str(Path.home() / ".codex"))
    parser.add_argument("--color", default="never", choices=["always", "never", "auto"])
    parser.add_argument("--disable-multi-agent", action="store_true")
    parser.add_argument("--yoyo-command")
    parser.add_argument("--yoyo-arg", action="append", default=None)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    workspace = Path(args.workspace).resolve()
    task_file = Path(args.task_file).resolve()
    metrics_file = Path(args.metrics_file).resolve()
    commands_file = Path(args.commands_file).resolve()
    metrics_file.parent.mkdir(parents=True, exist_ok=True)
    runtime_dir = Path(
        tempfile.mkdtemp(prefix=f"yoyo-directed-{args.mode}-", dir=tempfile.gettempdir())
    ).resolve()

    task = load_task(task_file)
    commands = load_commands(commands_file)
    if not commands:
        parser.error("--commands-file must contain at least one engineer command")

    codex_home = prepare_codex_home(
        workspace=workspace,
        mode=args.mode,
        base_codex_home=Path(args.base_codex_home).expanduser().resolve(),
        runtime_dir=runtime_dir,
        model=args.model,
        reasoning_effort=args.reasoning_effort,
        yoyo_command=args.yoyo_command,
        yoyo_args=args.yoyo_arg,
    )

    raw_jsonl_path = runtime_dir / f"codex-{args.mode}.jsonl"
    raw_stderr_path = runtime_dir / f"codex-{args.mode}.stderr.log"
    last_message_path = runtime_dir / f"codex-{args.mode}.last-message.txt"
    plan_path = runtime_dir / f"codex-{args.mode}.commands.json"
    plan_path.write_text(json.dumps(commands, indent=2) + "\n")

    if args.dry_run:
        metrics = {
            "source": "dry_run",
            "tool_calls": 0,
            "retries": 0,
            "notes": [
                f"mode={args.mode}",
                f"workspace={workspace}",
                f"commands={len(commands)}",
                f"codex_home={codex_home}",
                f"runtime_dir={runtime_dir}",
                f"commands_file={commands_file}",
            ],
        }
        metrics_file.write_text(json.dumps(metrics, indent=2) + "\n")
        return 0

    stdout_lines: list[str] = []
    stderr_chunks: list[str] = []
    append_output = False
    parsed_last_message: str | None = None
    returncode = 0
    prior_result: str | None = None

    for idx, step in enumerate(commands):
        prompt = build_step_prompt(
            task,
            args.mode,
            step["command"],
            step_index=idx + 1,
            total_steps=len(commands),
            prior_result=prior_result,
        )
        returncode, new_stdout_lines, new_stderr_text = run_codex(
            codex_bin=args.codex_bin,
            workspace=workspace,
            codex_home=codex_home,
            prompt=prompt,
            mode=args.mode,
            model=args.model,
            color=args.color,
            enable_multi_agent=not args.disable_multi_agent,
            last_message_path=last_message_path,
            raw_jsonl_path=raw_jsonl_path,
            raw_stderr_path=raw_stderr_path,
            resume_last=False,
            append_output=append_output,
        )
        append_output = True
        stdout_lines.extend(new_stdout_lines)
        stderr_chunks.append(new_stderr_text)
        _, parsed_last_message = parse_codex_jsonl(stdout_lines)
        prior_result = parsed_last_message

    stderr_text = "".join(stderr_chunks)
    metrics, parsed_last_message = parse_codex_jsonl(stdout_lines)
    metrics["notes"].extend(
        [
            f"returncode={returncode}",
            f"commands={len(commands)}",
            "directed_runner_mode=fresh_exec_per_step",
            f"commands_file={commands_file}",
            f"runtime_dir={runtime_dir}",
            f"raw_jsonl={raw_jsonl_path}",
            f"raw_stderr={raw_stderr_path}",
        ]
    )
    if args.mode == "treatment":
        yoyo_calls = next((note for note in metrics["notes"] if note.startswith("yoyo_tool_calls=")), None)
        if yoyo_calls is None:
            metrics["notes"].append("treatment made no yoyo MCP calls")

    metrics_file.write_text(json.dumps(metrics, indent=2) + "\n")
    if parsed_last_message:
        last_message_path.write_text(parsed_last_message)
        print(parsed_last_message)
    if stderr_text.strip():
        print(stderr_text, end="")
    return returncode


if __name__ == "__main__":
    raise SystemExit(main())
