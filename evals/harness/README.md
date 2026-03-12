# Eval Harness

`evals/harness/main.go` now supports four flows:

- `--setup`: clone a punctured repo fixture and print the working dir
- `--score`: run the task's test command in an existing working dir
- `--compare`: run the same task in `control` and `treatment` dirs, execute one command per condition, then compare tests and diffs
- `--all`: set up every task under a directory

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
