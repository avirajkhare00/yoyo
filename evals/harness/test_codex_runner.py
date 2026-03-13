import json
import tempfile
import unittest
from pathlib import Path

from codex_runner import build_prompt, parse_codex_jsonl, prepare_codex_home, requests_guidance


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
        self.assertIn("Do not ask for additional product or implementation guidance.", control)
        self.assertIn("continue investigating the repository", treatment)
        self.assertIn("Never stop to ask the evaluator what to do next.", control)
        self.assertIn("Extra note.", treatment)

    def test_requests_guidance_only_for_real_guidance_prompts(self):
        self.assertTrue(requests_guidance("What should I do next?"))
        self.assertTrue(requests_guidance("Can you clarify what you want me to change?"))
        self.assertFalse(requests_guidance("I fixed the regression."))
        self.assertFalse(requests_guidance("I will run the test again now."))

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
                runtime_dir=out / "control-runtime",
                model=None,
                reasoning_effort=None,
                yoyo_command=None,
                yoyo_args=None,
            )
            treatment_home = prepare_codex_home(
                workspace=tmp / "repo",
                mode="treatment",
                base_codex_home=base,
                runtime_dir=out / "treatment-runtime",
                model=None,
                reasoning_effort=None,
                yoyo_command=None,
                yoyo_args=None,
            )

            control_cfg = (control_home / "config.toml").read_text()
            treatment_cfg = (treatment_home / "config.toml").read_text()

            self.assertNotIn("[mcp_servers.yoyo]", control_cfg)
            self.assertIn("[mcp_servers.yoyo]", treatment_cfg)
            self.assertIn('command = "/usr/local/bin/yoyo"', treatment_cfg)
            self.assertTrue(str(control_home).startswith(str(out / "control-runtime")))
            self.assertTrue(str(treatment_home).startswith(str(out / "treatment-runtime")))

    def test_prepare_codex_home_uses_explicit_yoyo_override(self):
        with tempfile.TemporaryDirectory() as td:
            tmp = Path(td)
            base = tmp / "base"
            out = tmp / "out"
            base.mkdir()
            out.mkdir()
            (base / "auth.json").write_text('{"token":"x"}')
            (base / "config.toml").write_text('model = "gpt-5.4"\n')

            treatment_home = prepare_codex_home(
                workspace=tmp / "repo",
                mode="treatment",
                base_codex_home=base,
                runtime_dir=out / "runtime",
                model=None,
                reasoning_effort=None,
                yoyo_command="/tmp/yoyo-v1.8.1",
                yoyo_args=["--mcp-server"],
            )

            treatment_cfg = (treatment_home / "config.toml").read_text()
            self.assertIn('command = "/tmp/yoyo-v1.8.1"', treatment_cfg)
            self.assertIn('args = ["--mcp-server"]', treatment_cfg)


if __name__ == "__main__":
    unittest.main()
