// yoyo eval harness — real codebase puncture mode.
//
// Flows:
//
//	setup:   clone repo → apply puncture patch → run tests → emit working dir
//	score:   run tests in working dir → report pass/fail → write JSON result
//	compare: clone twice (control/treatment) → run agent commands → score tests/diff
//	all:     setup all tasks, print dirs
//
// Usage:
//
//	go run main.go --setup   --task ../tasks/httprouter
//	go run main.go --score   --dir /tmp/yoyo-eval-httprouter-go-001 --results ../../results
//	go run main.go --compare --task ../tasks/httprouter \
//	  --control-cmd 'claude --dangerously-skip-permissions "fix the bug"' \
//	  --treatment-cmd 'claude --dangerously-skip-permissions "fix the bug using yoyo MCP"'
package main

import (
	"bytes"
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"
)

type Task struct {
	ID        string   `json:"id"`
	Name      string   `json:"name"`
	Language  string   `json:"language"`
	Repo      string   `json:"repo"`
	Commit    string   `json:"commit"`
	TestCmd   []string `json:"test_cmd"`
	Punctures int      `json:"punctures"`
	Patch     string   `json:"patch"`
	Hints     []string `json:"hints"`
}

type SetupResult struct {
	Task    *Task
	Dir     string
	Before  TestResult
	Elapsed time.Duration
	Err     string
}

type TestResult struct {
	Passed int    `json:"passed"`
	Failed int    `json:"failed"`
	Output string `json:"output,omitempty"`
	Err    string `json:"err,omitempty"`
}

// ScoreRecord is written to evals/results/<timestamp>.json
type ScoreRecord struct {
	Timestamp string       `json:"timestamp"`
	Tasks     []TaskScore  `json:"tasks"`
	Summary   ScoreSummary `json:"summary"`
}

type TaskScore struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	Language  string `json:"language"`
	Punctures int    `json:"punctures"`
	Pass      bool   `json:"pass"`
	Failed    int    `json:"failed"`
	Passed    int    `json:"passed"`
}

type ScoreSummary struct {
	Total  int     `json:"total"`
	Passed int     `json:"passed"`
	Failed int     `json:"failed"`
	Score  float64 `json:"score"`
}

type CompareRecord struct {
	Timestamp string           `json:"timestamp"`
	Task      *Task            `json:"task"`
	Control   CompareCondition `json:"control"`
	Treatment CompareCondition `json:"treatment"`
	Summary   CompareSummary   `json:"summary"`
}

type CompareCondition struct {
	Mode    string       `json:"mode"`
	Dir     string       `json:"dir"`
	Command string       `json:"command,omitempty"`
	Before  TestResult   `json:"before"`
	After   TestResult   `json:"after"`
	Runner  RunnerResult `json:"runner"`
	Diff    DiffStat     `json:"diff"`
	Metrics AgentMetrics `json:"metrics"`
	Pass    bool         `json:"pass"`
}

type RunnerResult struct {
	ExitCode   int    `json:"exit_code"`
	ElapsedMs  int64  `json:"elapsed_ms"`
	StdoutPath string `json:"stdout_path,omitempty"`
	StderrPath string `json:"stderr_path,omitempty"`
	TimedOut   bool   `json:"timed_out,omitempty"`
	Err        string `json:"err,omitempty"`
}

type DiffStat struct {
	FilesChanged int      `json:"files_changed"`
	LinesAdded   int      `json:"lines_added"`
	LinesDeleted int      `json:"lines_deleted"`
	ChangedFiles []string `json:"changed_files,omitempty"`
}

type AgentMetrics struct {
	Source           string   `json:"source"`
	ToolCalls        *int     `json:"tool_calls,omitempty"`
	Retries          *int     `json:"retries,omitempty"`
	WrongEdits       *int     `json:"wrong_edits,omitempty"`
	HallucinatedAPIs *int     `json:"hallucinated_apis,omitempty"`
	Notes            []string `json:"notes,omitempty"`
}

type CompareSummary struct {
	Winner string   `json:"winner"`
	Reason string   `json:"reason"`
	Notes  []string `json:"notes,omitempty"`
}

func main() {
	setup := flag.Bool("setup", false, "clone repo, apply patch, run tests, print working dir")
	score := flag.Bool("score", false, "run tests in --dir and report score")
	compare := flag.Bool("compare", false, "run control/treatment commands in isolated punctured repos and compare outcomes")
	all := flag.String("all", "", "run setup on all task dirs under this path")
	taskDir := flag.String("task", "", "path to a task directory")
	dir := flag.String("dir", "", "working directory (for --score)")
	results := flag.String("results", "", "directory to write JSON result files (optional)")
	controlCmd := flag.String("control-cmd", "", "shell command for control condition")
	treatmentCmd := flag.String("treatment-cmd", "", "shell command for treatment condition")
	timeout := flag.Duration("timeout", 45*time.Minute, "max runtime per condition in compare mode")
	cleanup := flag.Bool("cleanup", false, "remove compare workdirs after writing the report")
	flag.Parse()

	switch {
	case *score:
		if *dir == "" {
			fatalf("--score requires --dir")
		}
		task, err := loadTaskFromDir(*dir)
		if err != nil {
			fatalf("load task: %v", err)
		}
		r := runTests(*dir, task.TestCmd)
		printScore(task, r)
		if *results != "" {
			writeResult(*results, []TaskScore{taskScoreFrom(task, r)})
		}
		if r.Failed > 0 {
			os.Exit(1)
		}

	case *compare:
		if *taskDir == "" {
			fatalf("--compare requires --task")
		}
		if strings.TrimSpace(*controlCmd) == "" || strings.TrimSpace(*treatmentCmd) == "" {
			fatalf("--compare requires both --control-cmd and --treatment-cmd")
		}
		record, err := runCompare(*taskDir, *controlCmd, *treatmentCmd, *timeout)
		if err != nil {
			fatalf("compare: %v", err)
		}
		printCompare(record)
		if *results != "" {
			writeCompareResult(*results, record)
		}
		if *cleanup {
			os.RemoveAll(record.Control.Dir)
			os.RemoveAll(record.Treatment.Dir)
		}
		if !record.Treatment.Pass {
			os.Exit(1)
		}

	case *all != "":
		entries, err := os.ReadDir(*all)
		if err != nil {
			fatalf("read dir: %v", err)
		}
		for _, e := range entries {
			if !e.IsDir() {
				continue
			}
			p := filepath.Join(*all, e.Name())
			if _, err := os.Stat(filepath.Join(p, "task.json")); err == nil {
				r := setupTask(p)
				printSetupResult(r)
			}
		}

	case *setup:
		if *taskDir == "" {
			fatalf("--setup requires --task")
		}
		r := setupTask(*taskDir)
		printSetupResult(r)
		if r.Err != "" {
			os.Exit(1)
		}

	default:
		flag.Usage()
		os.Exit(1)
	}
}

func setupTask(taskDir string) SetupResult {
	return setupTaskWithSuffix(taskDir, "")
}

func setupTaskWithSuffix(taskDir, suffix string) SetupResult {
	task, err := loadTask(taskDir)
	if err != nil {
		return SetupResult{Err: err.Error()}
	}

	start := time.Now()

	workDir := filepath.Join(os.TempDir(), "yoyo-eval-"+task.ID)
	if suffix != "" {
		workDir += "-" + suffix
	}
	if err := os.RemoveAll(workDir); err != nil {
		return SetupResult{Task: task, Err: fmt.Sprintf("clean workdir: %v", err)}
	}

	cloneArgs := []string{"clone", "--depth", "1"}
	if task.Commit != "" {
		cloneArgs = append(cloneArgs, "--branch", task.Commit)
	}
	cloneArgs = append(cloneArgs, task.Repo, workDir)

	if out, err := exec.Command("git", cloneArgs...).CombinedOutput(); err != nil {
		return SetupResult{Task: task, Err: fmt.Sprintf("clone: %v\n%s", err, out)}
	}

	patchPath, err := filepath.Abs(filepath.Join(taskDir, task.Patch))
	if err != nil {
		return SetupResult{Task: task, Err: fmt.Sprintf("patch path: %v", err)}
	}
	patchData, err := os.ReadFile(patchPath)
	if err != nil {
		return SetupResult{Task: task, Err: fmt.Sprintf("read patch: %v", err)}
	}
	applyCmd := exec.Command("git", "apply", "-")
	applyCmd.Dir = workDir
	applyCmd.Stdin = strings.NewReader(string(patchData))
	if out, err := applyCmd.CombinedOutput(); err != nil {
		return SetupResult{Task: task, Err: fmt.Sprintf("apply patch: %v\n%s", err, out)}
	}

	taskData, _ := json.MarshalIndent(task, "", "  ")
	os.WriteFile(filepath.Join(workDir, ".yoyo-task.json"), taskData, 0o644)

	before := runTests(workDir, task.TestCmd)

	return SetupResult{
		Task:    task,
		Dir:     workDir,
		Before:  before,
		Elapsed: time.Since(start),
	}
}

func runTests(dir string, cmd []string) TestResult {
	if len(cmd) >= 2 && cmd[0] == "go" && cmd[1] == "test" {
		injected := make([]string, 0, len(cmd)+1)
		injected = append(injected, cmd[0], cmd[1], "-v")
		injected = append(injected, cmd[2:]...)
		cmd = injected
	}
	c := exec.Command(cmd[0], cmd[1:]...)
	c.Dir = dir
	out, err := c.CombinedOutput()
	output := string(out)

	result := TestResult{Output: output}
	if err != nil {
		result.Err = err.Error()
	}

	for _, line := range strings.Split(output, "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "--- FAIL") || strings.HasPrefix(line, "FAIL\t") {
			result.Failed++
		}
		if strings.HasPrefix(line, "--- PASS") {
			result.Passed++
		}
		if strings.Contains(line, "test result:") {
			var p, f int
			if n, _ := fmt.Sscanf(line, "test result: ok. %d passed", &p); n == 1 {
				result.Passed += p
			}
			if n, _ := fmt.Sscanf(line, "test result: FAILED. %d passed; %d failed", &p, &f); n == 2 {
				result.Passed += p
				result.Failed += f
			}
		}
	}

	if result.Err != "" && result.Failed == 0 && result.Passed == 0 {
		result.Failed = 1
	}

	return result
}

func runCompare(taskDir, controlCmd, treatmentCmd string, timeout time.Duration) (*CompareRecord, error) {
	controlSetup := setupTaskWithSuffix(taskDir, "control")
	if controlSetup.Err != "" {
		return nil, fmt.Errorf("control setup failed: %s", controlSetup.Err)
	}
	treatmentSetup := setupTaskWithSuffix(taskDir, "treatment")
	if treatmentSetup.Err != "" {
		return nil, fmt.Errorf("treatment setup failed: %s", treatmentSetup.Err)
	}

	control := runCondition("control", controlSetup, controlCmd, timeout)
	treatment := runCondition("treatment", treatmentSetup, treatmentCmd, timeout)

	return &CompareRecord{
		Timestamp: time.Now().UTC().Format(time.RFC3339),
		Task:      controlSetup.Task,
		Control:   control,
		Treatment: treatment,
		Summary:   summarizeCompare(control, treatment),
	}, nil
}

func runCondition(mode string, setup SetupResult, command string, timeout time.Duration) CompareCondition {
	expanded := expandCommand(command, setup.Task, setup.Dir, mode)
	logDir := filepath.Join(setup.Dir, ".yoyo-eval")
	_ = os.MkdirAll(logDir, 0o755)
	harnessDir, _ := os.Getwd()

	stdoutPath := filepath.Join(logDir, mode+".stdout.log")
	stderrPath := filepath.Join(logDir, mode+".stderr.log")
	metricsPath := filepath.Join(logDir, mode+".metrics.json")

	runner := runShellCommand(setup.Dir, expanded, timeout, stdoutPath, stderrPath, map[string]string{
		"YOYO_EVAL_DIR":          setup.Dir,
		"YOYO_EVAL_MODE":         mode,
		"YOYO_EVAL_TASK_ID":      setup.Task.ID,
		"YOYO_EVAL_TASK_NAME":    setup.Task.Name,
		"YOYO_EVAL_TASK_FILE":    filepath.Join(setup.Dir, ".yoyo-task.json"),
		"YOYO_EVAL_METRICS_FILE": metricsPath,
		"YOYO_EVAL_HARNESS_DIR":  harnessDir,
		"YOYO_EVAL_HINTS":        strings.Join(setup.Task.Hints, "\n"),
	})

	after := runTests(setup.Dir, setup.Task.TestCmd)
	diff := gitDiffStat(setup.Dir)
	metrics := loadMetrics(metricsPath, stdoutPath, stderrPath)

	return CompareCondition{
		Mode:    mode,
		Dir:     setup.Dir,
		Command: expanded,
		Before:  setup.Before,
		After:   after,
		Runner:  runner,
		Diff:    diff,
		Metrics: metrics,
		Pass:    after.Failed == 0 && after.Err == "",
	}
}

func runShellCommand(dir, command string, timeout time.Duration, stdoutPath, stderrPath string, extraEnv map[string]string) RunnerResult {
	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()

	cmd := exec.CommandContext(ctx, "/bin/sh", "-lc", command)
	cmd.Dir = dir

	env := append([]string{}, os.Environ()...)
	for k, v := range extraEnv {
		env = append(env, k+"="+v)
	}
	cmd.Env = env

	var stdoutBuf bytes.Buffer
	var stderrBuf bytes.Buffer
	cmd.Stdout = &stdoutBuf
	cmd.Stderr = &stderrBuf

	start := time.Now()
	err := cmd.Run()
	elapsed := time.Since(start)

	_ = os.WriteFile(stdoutPath, stdoutBuf.Bytes(), 0o644)
	_ = os.WriteFile(stderrPath, stderrBuf.Bytes(), 0o644)

	result := RunnerResult{
		ElapsedMs:  elapsed.Milliseconds(),
		StdoutPath: stdoutPath,
		StderrPath: stderrPath,
	}

	if err == nil {
		return result
	}

	result.Err = err.Error()
	if ctx.Err() == context.DeadlineExceeded {
		result.TimedOut = true
		result.ExitCode = -1
		return result
	}
	if exitErr, ok := err.(*exec.ExitError); ok {
		result.ExitCode = exitErr.ExitCode()
		return result
	}
	result.ExitCode = -1
	return result
}

func expandCommand(command string, task *Task, dir, mode string) string {
	replacer := strings.NewReplacer(
		"{dir}", dir,
		"{mode}", mode,
		"{task_id}", task.ID,
		"{task_name}", task.Name,
		"{language}", task.Language,
		"{repo}", task.Repo,
	)
	return replacer.Replace(command)
}

func gitDiffStat(dir string) DiffStat {
	stat := DiffStat{}

	out, err := exec.Command("git", "-C", dir, "diff", "--numstat").CombinedOutput()
	if err == nil {
		for _, line := range strings.Split(strings.TrimSpace(string(out)), "\n") {
			if strings.TrimSpace(line) == "" {
				continue
			}
			parts := strings.Split(line, "\t")
			if len(parts) < 3 {
				continue
			}
			stat.FilesChanged++
			if n, err := strconv.Atoi(parts[0]); err == nil {
				stat.LinesAdded += n
			}
			if n, err := strconv.Atoi(parts[1]); err == nil {
				stat.LinesDeleted += n
			}
			stat.ChangedFiles = append(stat.ChangedFiles, parts[2])
		}
	}

	if len(stat.ChangedFiles) == 0 {
		names, err := exec.Command("git", "-C", dir, "diff", "--name-only").CombinedOutput()
		if err == nil {
			for _, name := range strings.Split(strings.TrimSpace(string(names)), "\n") {
				if strings.TrimSpace(name) != "" {
					stat.ChangedFiles = append(stat.ChangedFiles, name)
				}
			}
			stat.FilesChanged = len(stat.ChangedFiles)
		}
	}

	sort.Strings(stat.ChangedFiles)
	return stat
}

func loadMetrics(metricsPath, stdoutPath, stderrPath string) AgentMetrics {
	if data, err := os.ReadFile(metricsPath); err == nil {
		var metrics AgentMetrics
		if json.Unmarshal(data, &metrics) == nil {
			if metrics.Source == "" {
				metrics.Source = "runner"
			}
			return metrics
		}
	}

	stdout, _ := os.ReadFile(stdoutPath)
	stderr, _ := os.ReadFile(stderrPath)
	return heuristicMetrics(string(stdout) + "\n" + string(stderr))
}

func heuristicMetrics(log string) AgentMetrics {
	lower := strings.ToLower(log)
	toolCalls := 0
	for _, needle := range []string{
		"mcp__yoyo__",
		"grep",
		"rg ",
		"apply_patch",
		"read(",
		"edit(",
		"search(",
		"inspect(",
		"change(",
		"impact(",
		"map(",
		"routes(",
		"health(",
		"boot(",
		"index(",
	} {
		toolCalls += strings.Count(lower, needle)
	}

	retries := strings.Count(lower, "retry") +
		strings.Count(lower, "re-try") +
		strings.Count(lower, "try again")

	return AgentMetrics{
		Source:    "heuristic",
		ToolCalls: intPtr(toolCalls),
		Retries:   intPtr(retries),
		Notes: []string{
			"tool_calls and retries were inferred from runner stdout/stderr",
			"wrong_edits and hallucinated_apis stay empty unless the runner writes YOYO_EVAL_METRICS_FILE",
		},
	}
}

func summarizeCompare(control, treatment CompareCondition) CompareSummary {
	switch {
	case treatment.Pass && !control.Pass:
		return CompareSummary{
			Winner: "treatment",
			Reason: "treatment reached passing tests while control did not",
		}
	case control.Pass && !treatment.Pass:
		return CompareSummary{
			Winner: "control",
			Reason: "control reached passing tests while treatment did not",
		}
	case treatment.After.Failed < control.After.Failed:
		return CompareSummary{
			Winner: "treatment",
			Reason: "treatment left fewer failing tests",
			Notes:  []string{"both conditions either passed or failed; winner chosen by remaining failing tests"},
		}
	case control.After.Failed < treatment.After.Failed:
		return CompareSummary{
			Winner: "control",
			Reason: "control left fewer failing tests",
			Notes:  []string{"both conditions either passed or failed; winner chosen by remaining failing tests"},
		}
	case treatment.Diff.LinesAdded+treatment.Diff.LinesDeleted < control.Diff.LinesAdded+control.Diff.LinesDeleted:
		return CompareSummary{
			Winner: "treatment",
			Reason: "same test outcome, but treatment produced the smaller final diff",
		}
	case control.Diff.LinesAdded+control.Diff.LinesDeleted < treatment.Diff.LinesAdded+treatment.Diff.LinesDeleted:
		return CompareSummary{
			Winner: "control",
			Reason: "same test outcome, but control produced the smaller final diff",
		}
	default:
		return CompareSummary{
			Winner: "tie",
			Reason: "both conditions ended with the same test outcome and similar diff size",
		}
	}
}

func intPtr(v int) *int {
	return &v
}

func taskScoreFrom(task *Task, r TestResult) TaskScore {
	return TaskScore{
		ID:        task.ID,
		Name:      task.Name,
		Language:  task.Language,
		Punctures: task.Punctures,
		Pass:      r.Failed == 0 && r.Err == "",
		Failed:    r.Failed,
		Passed:    r.Passed,
	}
}

func writeResult(resultsDir string, scores []TaskScore) {
	if err := os.MkdirAll(resultsDir, 0o755); err != nil {
		fmt.Fprintf(os.Stderr, "warn: mkdir results: %v\n", err)
		return
	}
	summary := ScoreSummary{Total: len(scores)}
	for _, s := range scores {
		if s.Pass {
			summary.Passed++
		} else {
			summary.Failed++
		}
	}
	if summary.Total > 0 {
		summary.Score = float64(summary.Passed) / float64(summary.Total)
	}
	rec := ScoreRecord{
		Timestamp: time.Now().UTC().Format("2006-01-02T15:04:05Z"),
		Tasks:     scores,
		Summary:   summary,
	}
	taskID := ""
	if len(scores) == 1 {
		taskID = "-" + scores[0].ID
	}
	fname := filepath.Join(resultsDir, time.Now().UTC().Format("2006-01-02-150405")+taskID+".json")
	data, _ := json.MarshalIndent(rec, "", "  ")
	if err := os.WriteFile(fname, data, 0o644); err != nil {
		fmt.Fprintf(os.Stderr, "warn: write result: %v\n", err)
		return
	}
	fmt.Printf("result written: %s\n", fname)
}

func writeCompareResult(resultsDir string, rec *CompareRecord) {
	if err := os.MkdirAll(resultsDir, 0o755); err != nil {
		fmt.Fprintf(os.Stderr, "warn: mkdir results: %v\n", err)
		return
	}
	fname := filepath.Join(resultsDir, time.Now().UTC().Format("2006-01-02-150405")+"-"+rec.Task.ID+"-compare.json")
	data, _ := json.MarshalIndent(rec, "", "  ")
	if err := os.WriteFile(fname, data, 0o644); err != nil {
		fmt.Fprintf(os.Stderr, "warn: write compare result: %v\n", err)
		return
	}
	fmt.Printf("compare result written: %s\n", fname)
}

func loadTask(dir string) (*Task, error) {
	data, err := os.ReadFile(filepath.Join(dir, "task.json"))
	if err != nil {
		return nil, err
	}
	var t Task
	return &t, json.Unmarshal(data, &t)
}

func loadTaskFromDir(dir string) (*Task, error) {
	data, err := os.ReadFile(filepath.Join(dir, ".yoyo-task.json"))
	if err != nil {
		return nil, err
	}
	var t Task
	return &t, json.Unmarshal(data, &t)
}

func printSetupResult(r SetupResult) {
	name := "unknown"
	if r.Task != nil {
		name = r.Task.Name
	}
	fmt.Printf("\n=== %s ===\n", name)
	if r.Err != "" {
		fmt.Printf("[ERROR] %s\n", r.Err)
		return
	}
	fmt.Printf("working dir : %s\n", r.Dir)
	fmt.Printf("punctures   : %d\n", r.Task.Punctures)
	fmt.Printf("setup time  : %v\n", r.Elapsed.Round(time.Millisecond))
	if r.Before.Failed > 0 || r.Before.Err != "" {
		fmt.Printf("tests broken: %d failing — punctures confirmed\n", r.Before.Failed)
	} else {
		fmt.Printf("WARNING: no test failures — patch may not have applied correctly\n")
		fmt.Println(r.Before.Output)
	}
	fmt.Printf("\nModel: run yoyo tools against %s, then:\n", r.Dir)
	fmt.Printf("  go run main.go --score --dir %s\n", r.Dir)
	if len(r.Task.Hints) > 0 {
		fmt.Println("\nHints:")
		for _, h := range r.Task.Hints {
			fmt.Printf("  - %s\n", h)
		}
	}
}

func printScore(task *Task, r TestResult) {
	fmt.Printf("\n=== score: %s ===\n", task.Name)
	if r.Failed == 0 && r.Err == "" {
		fmt.Printf("[PASS] all tests passing\n")
	} else {
		fmt.Printf("[FAIL] %d tests still failing\n", r.Failed)
		if r.Err != "" {
			fmt.Printf("test command error: %s\n", r.Err)
		}
		fmt.Println(r.Output)
	}
}

func printCompare(rec *CompareRecord) {
	fmt.Printf("\n=== compare: %s ===\n", rec.Task.Name)
	fmt.Printf("winner     : %s\n", rec.Summary.Winner)
	fmt.Printf("why        : %s\n", rec.Summary.Reason)

	for _, cond := range []CompareCondition{rec.Control, rec.Treatment} {
		fmt.Printf("\n[%s]\n", cond.Mode)
		fmt.Printf("dir        : %s\n", cond.Dir)
		fmt.Printf("command    : %s\n", cond.Command)
		fmt.Printf("tests      : before %d failing → after %d failing\n", cond.Before.Failed, cond.After.Failed)
		if cond.After.Err != "" {
			fmt.Printf("test error : %s\n", cond.After.Err)
		}
		fmt.Printf("runner     : exit=%d elapsed=%dms\n", cond.Runner.ExitCode, cond.Runner.ElapsedMs)
		if cond.Runner.TimedOut {
			fmt.Printf("timeout    : true\n")
		}
		fmt.Printf("diff       : %d files, +%d/-%d\n", cond.Diff.FilesChanged, cond.Diff.LinesAdded, cond.Diff.LinesDeleted)
		if cond.Metrics.ToolCalls != nil {
			fmt.Printf("tool calls : %d (%s)\n", *cond.Metrics.ToolCalls, cond.Metrics.Source)
		}
		if cond.Metrics.Retries != nil {
			fmt.Printf("retries    : %d (%s)\n", *cond.Metrics.Retries, cond.Metrics.Source)
		}
		if cond.Runner.StdoutPath != "" {
			fmt.Printf("stdout log : %s\n", cond.Runner.StdoutPath)
		}
		if cond.Runner.StderrPath != "" {
			fmt.Printf("stderr log : %s\n", cond.Runner.StderrPath)
		}
	}
}

func fatalf(format string, args ...any) {
	fmt.Fprintf(os.Stderr, "fatal: "+format+"\n", args...)
	os.Exit(1)
}
