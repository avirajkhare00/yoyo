# Eval Harness

`evals/harness/main.go` now supports four flows:

- `--setup`: clone a punctured repo fixture and print the working dir
- `--score`: run the task's test command in an existing working dir
- `--compare`: run the same task in `control` and `treatment` dirs, execute one command per condition, then compare tests and diffs
- `--all`: set up every task under a directory

This harness is currently best treated as a **Tier 0 smoke-test runner**. It is useful for regression checks and tag comparisons, but puncture tasks are not the primary benchmark for day-to-day engineering value. See [`evals/README.md`](/Users/avirajkhare/yoyo-stuff/yoyo/evals/README.md) for the tiered eval strategy and the realistic replacement suite.

## Directed runner

[`directed_codex_runner.py`](/Users/avirajkhare/yoyo-stuff/yoyo/evals/harness/directed_codex_runner.py) runs a sequence of engineer-issued commands in one Codex session.

It is intended for the directed tool-use benchmark, where the evaluator steers the workflow with commands such as:

- `Find the likely implementation area first.`
- `Which layer should own this fix and why?`
- `Make the minimal patch.`
- `Run the exact verification command.`

Pilot command files live under:

- [`evals/tasks/ripgrep-global-gitignore/commands/read_only.json`](/Users/avirajkhare/yoyo-stuff/yoyo/evals/tasks/ripgrep-global-gitignore/commands/read_only.json)
- [`evals/tasks/ripgrep-global-gitignore/commands/write_only.json`](/Users/avirajkhare/yoyo-stuff/yoyo/evals/tasks/ripgrep-global-gitignore/commands/write_only.json)
- [`evals/tasks/ripgrep-global-gitignore/commands/read_then_write.json`](/Users/avirajkhare/yoyo-stuff/yoyo/evals/tasks/ripgrep-global-gitignore/commands/read_then_write.json)

Additional write-only command files now exist for:

- [`evals/tasks/uuid/commands/write_only.json`](/Users/avirajkhare/yoyo-stuff/yoyo/evals/tasks/uuid/commands/write_only.json)
- [`evals/tasks/httprouter/commands/write_only.json`](/Users/avirajkhare/yoyo-stuff/yoyo/evals/tasks/httprouter/commands/write_only.json)
- [`evals/tasks/semver/commands/write_only.json`](/Users/avirajkhare/yoyo-stuff/yoyo/evals/tasks/semver/commands/write_only.json)

Grouped batch manifests:

- [`evals/tasks/directed_tool_use_first3.json`](/Users/avirajkhare/yoyo-stuff/yoyo/evals/tasks/directed_tool_use_first3.json)
- [`evals/tasks/directed_tool_use_write_batch.json`](/Users/avirajkhare/yoyo-stuff/yoyo/evals/tasks/directed_tool_use_write_batch.json)

Example after fixture setup:

```bash
cd evals/harness
python3 directed_codex_runner.py \
  --mode treatment \
  --workspace /tmp/yoyo-eval-ripgrep-global-gitignore-rust-001 \
  --task-file /tmp/yoyo-eval-ripgrep-global-gitignore-rust-001/.yoyo-task.json \
  --metrics-file /tmp/yoyo-eval-ripgrep-global-gitignore-rust-001/.yoyo-eval/directed-treatment-metrics.json \
  --commands-file ../tasks/ripgrep-global-gitignore/commands/read_then_write.json
```

## Compare mode

Example:

```bash
cd evals/harness
go run main.go --compare --task ../tasks/httprouter \
  --control-cmd 'python3 "$YOYO_EVAL_HARNESS_DIR/codex_runner.py" --mode control' \
  --treatment-cmd 'python3 "$YOYO_EVAL_HARNESS_DIR/codex_runner.py" --mode treatment' \
  --results ../results
```

Each condition gets its own isolated workdir:

- `/tmp/yoyo-eval-<task-id>-control`
- `/tmp/yoyo-eval-<task-id>-treatment`

The harness records:

- failing tests before and after the run
- runner exit code and elapsed time
- final git diff size and changed files
- optional runner-supplied metrics

## Runner environment

In compare mode, each command receives:

- `YOYO_EVAL_DIR`
- `YOYO_EVAL_MODE`
- `YOYO_EVAL_TASK_ID`
- `YOYO_EVAL_TASK_NAME`
- `YOYO_EVAL_TASK_FILE`
- `YOYO_EVAL_METRICS_FILE`
- `YOYO_EVAL_HARNESS_DIR`
- `YOYO_EVAL_HINTS`

If your runner writes JSON to `YOYO_EVAL_METRICS_FILE`, the harness will include it in the report. Shape:

```json
{
  "tool_calls": 12,
  "retries": 2,
  "wrong_edits": 1,
  "hallucinated_apis": 0,
  "notes": ["optional notes from the runner"]
}
```

If no metrics file is written, the harness falls back to heuristic counts from stdout/stderr logs.

## Codex runner

[`codex_runner.py`](/Users/avirajkhare/yoyo-stuff/yoyo/evals/harness/codex_runner.py) is the default control/treatment wrapper for Codex-based runs.

- `control` mode creates an isolated `CODEX_HOME` with auth but no MCP servers configured
- `treatment` mode creates an isolated `CODEX_HOME` with only `yoyo` configured and enables `multi_agent`
- both modes parse `codex exec --json` output and write structured metrics into `YOYO_EVAL_METRICS_FILE`

Smoke check without spending a real run:

```bash
cd evals/harness
python3 "$PWD/codex_runner.py" --mode control --dry-run \
  --workspace /path/to/worktree --task-file /path/to/.yoyo-task.json --metrics-file /tmp/control-metrics.json
python3 "$PWD/codex_runner.py" --mode treatment --dry-run \
  --workspace /path/to/worktree --task-file /path/to/.yoyo-task.json --metrics-file /tmp/treatment-metrics.json
```

## Latest tags matrix

[`run_latest_tags.py`](/Users/avirajkhare/yoyo-stuff/yoyo/evals/harness/run_latest_tags.py) runs the same puncture task set against the latest release tags.

Dry run:

```bash
cd evals/harness
python3 run_latest_tags.py --dry-run
```

Real run against the latest 5 tags and the default task set (`uuid`, `httprouter`, `semver`):

```bash
cd evals/harness
python3 run_latest_tags.py
```

Outputs:

- built tag binaries cached under `/tmp/yoyo-tag-evals/<tag>/`
- per-tag compare reports under `evals/results/by-tag/<tag>/`
- one aggregate summary JSON under `evals/results/by-tag/`
