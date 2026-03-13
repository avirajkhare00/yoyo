#!/usr/bin/env python3
"""
Codex runner for the puncture compare harness.

This wrapper:
- reads task metadata from YOYO_EVAL_TASK_FILE
- prepares an isolated CODEX_HOME for control or treatment mode
- runs `codex exec --json`
- parses JSONL events into structured metrics
- writes metrics JSON to YOYO_EVAL_METRICS_FILE for the Go harness
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import threading
from collections import Counter
from pathlib import Path
from typing import Any

GUIDANCE_REPLY = "OK, continue."
GUIDANCE_PATTERNS = (
    re.compile(r"\bwhat should i do next\b", re.IGNORECASE),
    re.compile(r"\bhow would you like me to proceed\b", re.IGNORECASE),
    re.compile(r"\bdo you want me to\b", re.IGNORECASE),
    re.compile(r"\bcan you clarify\b", re.IGNORECASE),
    re.compile(r"\bneed more guidance\b", re.IGNORECASE),
    re.compile(r"\bplease provide\b.*\bguidance\b", re.IGNORECASE),
)


def load_task(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text())


def load_base_codex_config(codex_home: Path) -> dict[str, Any]:
    config_path = codex_home / "config.toml"
    if not config_path.exists():
        return {}
    text = config_path.read_text()
    try:
        import tomllib  # type: ignore

        return tomllib.loads(text)
    except ModuleNotFoundError:
        return parse_minimal_codex_config(text)


def parse_minimal_codex_config(text: str) -> dict[str, Any]:
    config: dict[str, Any] = {}
    current_section: str | None = None

    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue

        if line.startswith("[") and line.endswith("]"):
            current_section = line[1:-1]
            continue

        if "=" not in line:
            continue

        key, value = [part.strip() for part in line.split("=", 1)]

        if current_section is None:
            parsed = parse_toml_scalar(value)
            if parsed is not None:
                config[key] = parsed
            continue

        if current_section == "mcp_servers.yoyo":
            yoyo = config.setdefault("mcp_servers", {}).setdefault("yoyo", {})
            parsed = parse_toml_scalar(value)
            if parsed is not None:
                yoyo[key] = parsed

    return config


def parse_toml_scalar(value: str) -> Any:
    if value.startswith('"') and value.endswith('"'):
        return value[1:-1]
    if value.startswith("[") and value.endswith("]"):
        items = re.findall(r'"([^"]*)"', value)
        return items
    return None


def fallback_yoyo_config() -> dict[str, Any]:
    yoyo = shutil.which("yoyo")
    if not yoyo:
        raise RuntimeError("treatment mode requires yoyo on PATH or [mcp_servers.yoyo] in ~/.codex/config.toml")
    return {"command": yoyo, "args": ["--mcp-server"]}


def render_config(
    *,
    workspace: Path,
    base_config: dict[str, Any],
    mode: str,
    model: str | None,
    reasoning_effort: str | None,
    yoyo_command: str | None,
    yoyo_args: list[str] | None,
) -> str:
    lines: list[str] = []
    cfg_model = model or base_config.get("model")
    if cfg_model:
        lines.append(f'model = "{cfg_model}"')

    cfg_reasoning = reasoning_effort or base_config.get("model_reasoning_effort")
    if cfg_reasoning:
        lines.append(f'model_reasoning_effort = "{cfg_reasoning}"')

    lines.extend([
        "",
        f'[projects."{workspace}"]',
        'trust_level = "trusted"',
    ])

    if mode == "treatment":
        mcp_yoyo = (
            {"command": yoyo_command, "args": yoyo_args or ["--mcp-server"]}
            if yoyo_command
            else (
                base_config.get("mcp_servers", {}).get("yoyo")
                if isinstance(base_config.get("mcp_servers"), dict)
                else None
            )
        ) or fallback_yoyo_config()
        lines.extend([
            "",
            "[mcp_servers.yoyo]",
            f'command = "{mcp_yoyo["command"]}"',
        ])
        args = mcp_yoyo.get("args", [])
        if args:
            quoted = ", ".join(f'"{arg}"' for arg in args)
            lines.append(f"args = [{quoted}]")

    return "\n".join(lines) + "\n"


def prepare_codex_home(
    *,
    workspace: Path,
    mode: str,
    base_codex_home: Path,
    runtime_dir: Path,
    model: str | None,
    reasoning_effort: str | None,
    yoyo_command: str | None,
    yoyo_args: list[str] | None,
) -> Path:
    codex_home = runtime_dir / f"codex-home-{mode}"
    if codex_home.exists():
        shutil.rmtree(codex_home)
    codex_home.mkdir(parents=True, exist_ok=True)

    auth_src = base_codex_home / "auth.json"
    if auth_src.exists():
        shutil.copy2(auth_src, codex_home / "auth.json")

    base_config = load_base_codex_config(base_codex_home)
    config_text = render_config(
        workspace=workspace,
        base_config=base_config,
        mode=mode,
        model=model,
        reasoning_effort=reasoning_effort,
        yoyo_command=yoyo_command,
        yoyo_args=yoyo_args,
    )
    (codex_home / "config.toml").write_text(config_text)
    return codex_home


def build_prompt(task: dict[str, Any], mode: str, extra: str | None) -> str:
    hints = task.get("hints") or []
    prompt = [
        "You are repairing a punctured repository eval fixture.",
        "Modify only the current repository so the task test command passes.",
        "Start by reading .yoyo-task.json and checking the current git diff.",
        "Use the provided test command as the correctness oracle and run it before finishing.",
        "Do not ask for additional product or implementation guidance.",
        "If you are unsure what to do next, continue investigating the repository and run the next relevant local verification command.",
        "Never stop to ask the evaluator what to do next. Make the best reasonable assumption and continue.",
        "",
        f"Task ID: {task['id']}",
        f"Task name: {task['name']}",
        f"Language: {task['language']}",
        f"Test command: {' '.join(task['test_cmd'])}",
    ]
    if hints:
        prompt.append("Hints:")
        prompt.extend(f"- {hint}" for hint in hints)

    if mode == "control":
        prompt.extend(
            [
                "",
                "Use Codex built-in tools only. No MCP servers are configured for this run.",
            ]
        )
    else:
        prompt.extend(
            [
                "",
                "yoyo MCP is configured for this run.",
                "Before any repo exploration, call boot and index in parallel.",
                "Prefer yoyo search/inspect/impact/change over built-in repo exploration and edit tools when they fit.",
                "Treatment success is stronger if you actually use yoyo MCP tools.",
            ]
        )

    if extra:
        prompt.extend(["", extra.strip()])

    prompt.extend(["", "Return a concise summary of the fix at the end."])
    return "\n".join(prompt)


def parse_codex_jsonl(lines: list[str]) -> tuple[dict[str, Any], str | None]:
    counts: Counter[str] = Counter()
    shell_commands: list[str] = []
    mcp_calls: list[tuple[str, str]] = []
    last_message: str | None = None
    raw_events = 0

    for line in lines:
        text = line.strip()
        if not text:
            continue
        raw_events += 1
        try:
            event = json.loads(text)
        except json.JSONDecodeError:
            continue

        item = event.get("item")
        if not isinstance(item, dict):
            continue

        if event.get("type") != "item.completed":
            continue

        item_type = item.get("type")
        if item_type == "command_execution":
            counts["shell_tool_calls"] += 1
            command = item.get("command")
            if isinstance(command, str):
                shell_commands.append(command)
        elif item_type == "mcp_tool_call":
            counts["mcp_tool_calls"] += 1
            server = item.get("server")
            tool = item.get("tool")
            if server == "yoyo":
                counts["yoyo_tool_calls"] += 1
            if isinstance(server, str) and isinstance(tool, str):
                mcp_calls.append((server, tool))
        elif item_type == "agent_message":
            text_value = item.get("text")
            if isinstance(text_value, str):
                last_message = text_value

    shell_retry_count = sum(count - 1 for count in Counter(shell_commands).values() if count > 1)
    mcp_retry_count = sum(count - 1 for count in Counter(mcp_calls).values() if count > 1)

    notes = [
        f"shell_tool_calls={counts['shell_tool_calls']}",
        f"mcp_tool_calls={counts['mcp_tool_calls']}",
    ]
    if counts["yoyo_tool_calls"]:
        notes.append(f"yoyo_tool_calls={counts['yoyo_tool_calls']}")

    metrics = {
        "source": "codex_jsonl",
        "tool_calls": counts["shell_tool_calls"] + counts["mcp_tool_calls"],
        "retries": shell_retry_count + mcp_retry_count,
        "notes": notes + [f"parsed_events={raw_events}"],
    }
    return metrics, last_message


def requests_guidance(message: str | None) -> bool:
    if not message:
        return False
    if "?" not in message:
        return False
    return any(pattern.search(message) for pattern in GUIDANCE_PATTERNS)


def run_codex(
    *,
    codex_bin: str,
    workspace: Path,
    codex_home: Path,
    prompt: str,
    mode: str,
    model: str | None,
    color: str,
    enable_multi_agent: bool,
    last_message_path: Path,
    raw_jsonl_path: Path,
    raw_stderr_path: Path,
    resume_last: bool = False,
    append_output: bool = False,
) -> tuple[int, list[str], str]:
    if resume_last:
        cmd = [
            codex_bin,
            "exec",
            "resume",
            "--last",
            "--json",
            "--full-auto",
            "-o",
            str(last_message_path),
        ]
    else:
        cmd = [
            codex_bin,
            "exec",
            "--json",
            "--color",
            color,
            "--full-auto",
            "-C",
            str(workspace),
            "-o",
            str(last_message_path),
        ]
    if model:
        cmd.extend(["-m", model])
    if mode == "treatment" and enable_multi_agent:
        cmd.extend(["--enable", "multi_agent"])
    cmd.append(GUIDANCE_REPLY if resume_last else prompt)

    env = dict(os.environ)
    env["CODEX_HOME"] = str(codex_home)
    env.setdefault("OTEL_SDK_DISABLED", "true")
    env.setdefault("CODEX_DISABLE_TELEMETRY", "1")

    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
        env=env,
        cwd=str(workspace) if resume_last else None,
    )
    assert proc.stdout is not None
    assert proc.stderr is not None

    stdout_lines: list[str] = []
    stderr_lines: list[str] = []

    def pump(stream: Any, sink_path: Path, bucket: list[str]) -> None:
        mode = "a" if append_output else "w"
        with sink_path.open(mode) as sink:
            for line in stream:
                sink.write(line)
                sink.flush()
                bucket.append(line)

    stdout_thread = threading.Thread(
        target=pump,
        args=(proc.stdout, raw_jsonl_path, stdout_lines),
        daemon=True,
    )
    stderr_thread = threading.Thread(
        target=pump,
        args=(proc.stderr, raw_stderr_path, stderr_lines),
        daemon=True,
    )
    stdout_thread.start()
    stderr_thread.start()

    returncode = proc.wait()
    stdout_thread.join()
    stderr_thread.join()
    return returncode, stdout_lines, "".join(stderr_lines)


def main() -> int:
    parser = argparse.ArgumentParser(description="Codex runner for the yoyo puncture compare harness")
    parser.add_argument("--mode", required=True, choices=["control", "treatment"])
    parser.add_argument("--workspace", default=os.environ.get("YOYO_EVAL_DIR"))
    parser.add_argument("--task-file", default=os.environ.get("YOYO_EVAL_TASK_FILE"))
    parser.add_argument("--metrics-file", default=os.environ.get("YOYO_EVAL_METRICS_FILE"))
    parser.add_argument("--prompt-extra")
    parser.add_argument("--model")
    parser.add_argument("--reasoning-effort")
    parser.add_argument("--codex-bin", default=shutil.which("codex") or "codex")
    parser.add_argument("--base-codex-home", default=os.environ.get("CODEX_HOME") or str(Path.home() / ".codex"))
    parser.add_argument("--color", default="never", choices=["always", "never", "auto"])
    parser.add_argument("--disable-multi-agent", action="store_true")
    parser.add_argument("--yoyo-command")
    parser.add_argument("--yoyo-arg", action="append", default=None)
    parser.add_argument("--max-guidance-resumes", type=int, default=1)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    if not args.workspace or not args.task_file or not args.metrics_file:
        parser.error("--workspace, --task-file, and --metrics-file are required (or set YOYO_EVAL_* env vars)")

    workspace = Path(args.workspace).resolve()
    task_file = Path(args.task_file).resolve()
    metrics_file = Path(args.metrics_file).resolve()
    metrics_file.parent.mkdir(parents=True, exist_ok=True)
    runtime_dir = Path(
        tempfile.mkdtemp(prefix=f"yoyo-codex-{args.mode}-", dir=tempfile.gettempdir())
    ).resolve()

    task = load_task(task_file)
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
    prompt = build_prompt(task, args.mode, args.prompt_extra)

    prompt_path = runtime_dir / f"codex-{args.mode}.prompt.txt"
    prompt_path.write_text(prompt)
    config_path = codex_home / "config.toml"
    last_message_path = runtime_dir / f"codex-{args.mode}.last-message.txt"
    raw_jsonl_path = runtime_dir / f"codex-{args.mode}.jsonl"
    raw_stderr_path = runtime_dir / f"codex-{args.mode}.stderr.log"

    command_preview = [
        args.codex_bin,
        "exec",
        "--json",
        "--color",
        args.color,
        "--full-auto",
        "-C",
        str(workspace),
    ]
    if args.model:
        command_preview.extend(["-m", args.model])
    if args.mode == "treatment" and not args.disable_multi_agent:
        command_preview.extend(["--enable", "multi_agent"])
    command_preview.extend(["-o", str(last_message_path), "<prompt>"])

    if args.dry_run:
        metrics = {
            "source": "dry_run",
            "tool_calls": 0,
            "retries": 0,
            "notes": [
                f"mode={args.mode}",
                f"workspace={workspace}",
                f"codex_home={codex_home}",
                f"runtime_dir={runtime_dir}",
                f"config_path={config_path}",
                f"command={' '.join(shlex.quote(part) for part in command_preview)}",
            ],
        }
        metrics_file.write_text(json.dumps(metrics, indent=2) + "\n")
        print(json.dumps({"mode": args.mode, "prompt_path": str(prompt_path), "config_path": str(config_path)}))
        return 0

    stdout_lines: list[str] = []
    stderr_chunks: list[str] = []
    guidance_resumes = 0
    returncode = 0
    parsed_last_message: str | None = None
    while True:
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
            resume_last=guidance_resumes > 0,
            append_output=guidance_resumes > 0,
        )
        stdout_lines.extend(new_stdout_lines)
        stderr_chunks.append(new_stderr_text)
        _, parsed_last_message = parse_codex_jsonl(stdout_lines)
        if guidance_resumes >= args.max_guidance_resumes or not requests_guidance(parsed_last_message):
            break
        guidance_resumes += 1

    stderr_text = "".join(stderr_chunks)
    metrics, parsed_last_message = parse_codex_jsonl(stdout_lines)
    metrics["notes"].extend(
        [
            f"returncode={returncode}",
            f"runtime_dir={runtime_dir}",
            f"raw_jsonl={raw_jsonl_path}",
            f"raw_stderr={raw_stderr_path}",
        ]
    )
    if guidance_resumes:
        metrics["notes"].append(f"guidance_auto_resumes={guidance_resumes}")
    if args.mode == "treatment" and "yoyo_tool_calls=0" not in metrics["notes"]:
        yoyo_calls = next((note for note in metrics["notes"] if note.startswith("yoyo_tool_calls=")), None)
        if yoyo_calls is None:
            metrics["notes"].append("treatment made no yoyo MCP calls")

    if parsed_last_message and not last_message_path.exists():
        last_message_path.write_text(parsed_last_message)

    metrics_file.write_text(json.dumps(metrics, indent=2) + "\n")
    if parsed_last_message:
        print(parsed_last_message)

    if stderr_text.strip():
        print(stderr_text, file=sys.stderr, end="" if stderr_text.endswith("\n") else "\n")
    return returncode


if __name__ == "__main__":
    raise SystemExit(main())
