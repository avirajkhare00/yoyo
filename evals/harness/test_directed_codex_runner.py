import json
import tempfile
import unittest
from pathlib import Path

from directed_codex_runner import (
    build_step_prompt,
    command_is_read_only,
    command_is_write_focused,
    load_commands,
)


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

    def test_command_is_read_only_detects_no_edit_instructions(self):
        self.assertTrue(command_is_read_only("Find the likely files. Do not edit anything."))
        self.assertTrue(command_is_read_only("This is a read-only investigation."))
        self.assertFalse(command_is_read_only("Make the minimal patch."))

    def test_command_is_write_focused_detects_patch_language(self):
        self.assertTrue(command_is_write_focused("Make the minimal patch on that surface only."))
        self.assertTrue(command_is_write_focused("Rename the helper and update direct callers."))
        self.assertFalse(command_is_write_focused("Run the exact regression test command."))

    def test_editable_prompt_omits_autonomous_repair_language(self):
        task = {
            "id": "t1",
            "name": "fixture",
            "language": "rust",
            "test_cmd": ["cargo", "test"],
            "hints": [],
        }
        prompt = build_step_prompt(
            task,
            "treatment",
            "Find the likely files.",
            step_index=1,
            total_steps=3,
            prior_result=None,
        )

        self.assertIn("This is a directed tool-use eval.", prompt)
        self.assertIn("Step 1 of 3.", prompt)
        self.assertIn("Engineer command: Find the likely files.", prompt)
        self.assertIn("Before any repo exploration, call boot and index in parallel.", prompt)
        self.assertIn("This step may include edits if the engineer command asks for them.", prompt)
        self.assertIn("Task test command: cargo test", prompt)
        self.assertNotIn("repair the repo", prompt)

    def test_read_only_prompt_stops_after_bounded_answer(self):
        task = {
            "id": "t1",
            "name": "fixture",
            "language": "rust",
            "test_cmd": ["cargo", "test"],
            "hints": [],
        }
        prompt = build_step_prompt(
            task,
            "treatment",
            "Find the likely files. Do not edit anything.",
            step_index=1,
            total_steps=3,
            prior_result=None,
        )

        self.assertIn("This step is read-only.", prompt)
        self.assertIn("Do not try to make the task test command pass in this step.", prompt)
        self.assertIn("Budget: use at most 8 tool calls for this step, including boot/index.", prompt)
        self.assertIn("If the previous engineer-command result already gives enough evidence, answer directly instead of reopening the whole area.", prompt)
        self.assertIn("When you have the answer, stop immediately and report only the result for this step.", prompt)
        self.assertNotIn("Task test command: cargo test", prompt)

    def test_follow_up_prompt_includes_prior_result(self):
        task = {
            "id": "t1",
            "name": "fixture",
            "language": "rust",
            "test_cmd": ["cargo", "test"],
            "hints": [],
        }
        follow_up = build_step_prompt(
            task,
            "treatment",
            "Make the minimal patch.",
            step_index=2,
            total_steps=3,
            prior_result="Likely ownership is in the walker setup.",
        )

        self.assertIn("Do not inspect git history or hidden oracle material.", follow_up)
        self.assertIn("Engineer command: Make the minimal patch.", follow_up)
        self.assertIn("Previous engineer-command result:", follow_up)
        self.assertIn("Likely ownership is in the walker setup.", follow_up)
        self.assertIn("This is a write-focused step.", follow_up)
        self.assertIn("Use at most 2 additional confirming reads before the first edit.", follow_up)
        self.assertIn("Do not use script in this step.", follow_up)
        self.assertIn("Prefer direct inspect/search calls, then change.", follow_up)
        self.assertIn("If yoyo is available, use change for the edit instead of a raw patch path.", follow_up)


if __name__ == "__main__":
    unittest.main()
