import json
import tempfile
import unittest
from pathlib import Path

from directed_codex_runner import build_step_prompt, load_commands


class DirectedCodexRunnerTests(unittest.TestCase):
    def test_load_commands_accepts_strings_and_objects(self):
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "commands.json"
            path.write_text(json.dumps(["one", {"id": "two", "command": "two"}]))
            commands = load_commands(path)

        self.assertEqual(commands[0]["id"], "step-1")
        self.assertEqual(commands[0]["command"], "one")
        self.assertEqual(commands[1]["id"], "two")
        self.assertEqual(commands[1]["command"], "two")

    def test_prompts_include_directed_eval_contract(self):
        task = {
            "id": "t1",
            "name": "fixture",
            "language": "rust",
            "test_cmd": ["cargo", "test"],
            "hints": [],
        }
        initial = build_step_prompt(
            task,
            "treatment",
            "Find the likely files.",
            step_index=1,
            total_steps=3,
            prior_result=None,
        )
        follow_up = build_step_prompt(
            task,
            "treatment",
            "Make the minimal patch.",
            step_index=2,
            total_steps=3,
            prior_result="Likely ownership is in the walker setup.",
        )

        self.assertIn("This is a directed tool-use eval.", initial)
        self.assertIn("Step 1 of 3.", initial)
        self.assertIn("Engineer command: Find the likely files.", initial)
        self.assertIn("Do not inspect git history or hidden oracle material.", initial)
        self.assertIn("Engineer command: Make the minimal patch.", follow_up)
        self.assertIn("Previous engineer-command result:", follow_up)
        self.assertIn("Likely ownership is in the walker setup.", follow_up)


if __name__ == "__main__":
    unittest.main()
