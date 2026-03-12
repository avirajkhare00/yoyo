import json
import tempfile
import unittest
from pathlib import Path

from codex_runner import build_prompt, parse_codex_jsonl, prepare_codex_home


class CodexRunnerTests(unittest.TestCase):
    def test_parse_codex_jsonl_counts_shell_and_mcp_calls(self):
        lines = [
            json.dumps({"type": "thread.started", "thread_id": "abc"}),
            json.dumps(
                {
                    "type": "item.completed",
                    "item": {
                        "id": "item_0",
                        "type": "command_execution",
                        "command": "/bin/zsh -lc pwd",
                        "aggregated_output": "/tmp\n",
                        "exit_code": 0,
                        "status": "completed",
                    },
                }
            ),
            json.dumps(
                {
                    "type": "item.completed",
                    "item": {
                        "id": "item_1",
                        "type": "mcp_tool_call",
                        "server": "yoyo",
                        "tool": "boot",
                        "arguments": {"path": "/repo"},
                        "status": "completed",
                    },
                }
            ),
            json.dumps(
                {
                    "type": "item.completed",
                    "item": {
                        "id": "item_2",
                        "type": "mcp_tool_call",
                        "server": "yoyo",
                        "tool": "boot",
                        "arguments": {"path": "/repo"},
                        "status": "completed",
                    },
                }
            ),
            json.dumps(
                {
                    "type": "item.completed",
                    "item": {
                        "id": "item_3",
                        "type": "agent_message",
                        "text": "done",
                    },
                }
            ),
        ]

        metrics, last_message = parse_codex_jsonl(lines)

        self.assertEqual(metrics["source"], "codex_jsonl")
        self.assertEqual(metrics["tool_calls"], 3)
        self.assertEqual(metrics["retries"], 1)
        self.assertIn("shell_tool_calls=1", metrics["notes"])
        self.assertIn("mcp_tool_calls=2", metrics["notes"])
        self.assertIn("yoyo_tool_calls=2", metrics["notes"])
        self.assertEqual(last_message, "done")

    def test_build_prompt_differs_by_mode(self):
        task = {
            "id": "t1",
            "name": "fixture",
            "language": "go",
            "test_cmd": ["go", "test", "./..."],
            "hints": ["read the failing test"],
        }

        control = build_prompt(task, "control", None)
        treatment = build_prompt(task, "treatment", "Extra note.")

        self.assertIn("No MCP servers are configured", control)
        self.assertIn("call boot and index in parallel", treatment)
        self.assertIn("Extra note.", treatment)

    def test_prepare_codex_home_control_omits_mcp_and_treatment_keeps_yoyo(self):
        with tempfile.TemporaryDirectory() as td:
            tmp = Path(td)
            base = tmp / "base"
            out = tmp / "out"
            base.mkdir()
            out.mkdir()
            (base / "auth.json").write_text('{"token":"x"}')
            (base / "config.toml").write_text(
                '\n'.join(
                    [
                        'model = "gpt-5.4"',
                        'model_reasoning_effort = "high"',
                        '',
                        '[mcp_servers.yoyo]',
                        'command = "/usr/local/bin/yoyo"',
                        'args = ["--mcp-server"]',
                    ]
                )
                + '\n'
            )

            control_home = prepare_codex_home(
                workspace=tmp / "repo",
                mode="control",
                base_codex_home=base,
                out_dir=out,
                model=None,
                reasoning_effort=None,
            )
            treatment_home = prepare_codex_home(
                workspace=tmp / "repo",
                mode="treatment",
                base_codex_home=base,
                out_dir=out,
                model=None,
                reasoning_effort=None,
            )

            control_cfg = (control_home / "config.toml").read_text()
            treatment_cfg = (treatment_home / "config.toml").read_text()

            self.assertNotIn("[mcp_servers.yoyo]", control_cfg)
            self.assertIn("[mcp_servers.yoyo]", treatment_cfg)
            self.assertIn('command = "/usr/local/bin/yoyo"', treatment_cfg)


if __name__ == "__main__":
    unittest.main()
