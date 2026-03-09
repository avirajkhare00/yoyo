// yoyo eval harness — real codebase puncture mode.
//
// Flow:
//   setup: clone repo → apply puncture patch → run tests → emit working dir
//   score: run tests in working dir → report pass/fail → write JSON result
//   run:   setup + score in one pass (model must fix before scoring)
//
// Usage:
//   go run main.go --setup --task ../tasks/httprouter
//   go run main.go --score  --dir /tmp/yoyo-eval-httprouter-abc123 --results ../../results
//   go run main.go --all    ../tasks   (setup all tasks, print dirs)
package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
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
	Passed int
	Failed int
	Output string
	Err    string
}

// ScoreRecord is written to evals/results/<timestamp>.json
type ScoreRecord struct {
	Timestamp string        `json:"timestamp"`
	Tasks     []TaskScore   `json:"tasks"`
	Summary   ScoreSummary  `json:"summary"`
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

func main() {
	setup := flag.Bool("setup", false, "clone repo, apply patch, run tests, print working dir")
	score := flag.Bool("score", false, "run tests in --dir and report score")
	all := flag.String("all", "", "run setup on all task dirs under this path")
	taskDir := flag.String("task", "", "path to a task directory")
	dir := flag.String("dir", "", "working directory (for --score)")
	results := flag.String("results", "", "directory to write JSON result files (optional)")
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
	task, err := loadTask(taskDir)
	if err != nil {
		return SetupResult{Err: err.Error()}
	}

	start := time.Now()

	// Create deterministic working dir based on task ID
	workDir := filepath.Join(os.TempDir(), "yoyo-eval-"+task.ID)
	if err := os.RemoveAll(workDir); err != nil {
		return SetupResult{Task: task, Err: fmt.Sprintf("clean workdir: %v", err)}
	}

	// Clone
	cloneArgs := []string{"clone", "--depth", "1"}
	if task.Commit != "" {
		cloneArgs = append(cloneArgs, "--branch", task.Commit)
	}
	cloneArgs = append(cloneArgs, task.Repo, workDir)

	if out, err := exec.Command("git", cloneArgs...).CombinedOutput(); err != nil {
		return SetupResult{Task: task, Err: fmt.Sprintf("clone: %v\n%s", err, out)}
	}

	// Apply puncture patch
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

	// Write task.json into workdir so --score can find it
	taskData, _ := json.MarshalIndent(task, "", "  ")
	os.WriteFile(filepath.Join(workDir, ".yoyo-task.json"), taskData, 0o644)

	// Run tests — expect failures (confirms punctures land)
	before := runTests(workDir, task.TestCmd)

	return SetupResult{
		Task:    task,
		Dir:     workDir,
		Before:  before,
		Elapsed: time.Since(start),
	}
}

func runTests(dir string, cmd []string) TestResult {
	c := exec.Command(cmd[0], cmd[1:]...)
	c.Dir = dir
	out, err := c.CombinedOutput()
	output := string(out)

	result := TestResult{Output: output}
	if err != nil {
		result.Err = err.Error()
	}

	// Parse test results
	for _, line := range strings.Split(output, "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "--- FAIL") || strings.HasPrefix(line, "FAIL\t") {
			result.Failed++
		}
		if strings.HasPrefix(line, "--- PASS") {
			result.Passed++
		}
		// Rust: "test result: ok. N passed; M failed"
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

	return result
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
	if r.Before.Failed > 0 {
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
		fmt.Println(r.Output)
	}
}

func fatalf(format string, args ...any) {
	fmt.Fprintf(os.Stderr, "fatal: "+format+"\n", args...)
	os.Exit(1)
}
