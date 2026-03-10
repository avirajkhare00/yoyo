// token_benchmark — measures input token consumption: linux tools vs yoyo tools.
//
// For each task, both approaches gather context to answer a question about a codebase.
// That context is sent to gpt-4o-mini. We record prompt_tokens (from OpenAI usage) and
// score the answer against gold keywords.
//
// Usage:
//
//	OPENAI_API_KEY=sk-... go run . --repo /path/to/repo --out ../results
//	OPENAI_API_KEY=sk-... go run . --repo /path/to/repo --task caller-discovery
package main

import (
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

func main() {
	repo := flag.String("repo", "", "path to the repository to benchmark against (required)")
	outDir := flag.String("out", "../results", "directory to write JSON results")
	taskID := flag.String("task", "", "run a single task by ID (default: all)")
	yoyoBin := flag.String("yoyo", "", "path to yoyo binary (default: auto-detect)")
	flag.Parse()

	apiKey := os.Getenv("OPENAI_API_KEY")
	if apiKey == "" {
		fmt.Fprintln(os.Stderr, "error: OPENAI_API_KEY env var not set")
		os.Exit(1)
	}

	if *repo == "" {
		fmt.Fprintln(os.Stderr, "error: --repo is required")
		flag.Usage()
		os.Exit(1)
	}

	repoPath, err := filepath.Abs(*repo)
	if err != nil {
		fmt.Fprintf(os.Stderr, "error: bad repo path: %v\n", err)
		os.Exit(1)
	}

	bin := resolveYoyo(*yoyoBin)
	fmt.Printf("yoyo binary: %s\nrepo:        %s\nmodel:       gpt-4o-mini\n\n", bin, repoPath)

	// bake the index first so yoyo read-indexed tools work
	fmt.Print("baking index... ")
	out, _, bakeErr := runCmds([]string{bin + " bake --path {{REPO}}"}, repoPath)
	if bakeErr != nil {
		fmt.Fprintf(os.Stderr, "bake error: %v\n%s\n", bakeErr, out)
	} else {
		fmt.Println("done")
	}

	fmt.Print("discovering repo profile... ")
	profile, err := discoverRepo(bin, repoPath)
	if err != nil {
		fmt.Fprintf(os.Stderr, "error: %v\n", err)
		os.Exit(1)
	}

	tasks := buildTasks(bin, profile)
	if *taskID != "" {
		tasks = filterTasks(tasks, *taskID)
		if len(tasks) == 0 {
			fmt.Fprintf(os.Stderr, "error: no task with id %q\n", *taskID)
			os.Exit(1)
		}
	}

	var results []TaskResult
	for i, t := range tasks {
		fmt.Printf("[%d/%d] %s ... ", i+1, len(tasks), t.ID)
		r := runTask(t, repoPath, apiKey)
		if r.Err != "" {
			fmt.Printf("ERROR: %s\n", r.Err)
		} else {
			fmt.Printf("linux=%d tok  yoyo=%d tok  -%d%%\n",
				r.LinuxTokens, r.YoyoTokens, r.Reduction)
		}
		results = append(results, r)
	}

	summary := buildSummary(results)
	fmt.Println()
	printTable(results, summary)

	if err := os.MkdirAll(*outDir, 0755); err != nil {
		fmt.Fprintf(os.Stderr, "warn: could not create out dir: %v\n", err)
		return
	}
	outFile, err := saveReport(results, summary, repoPath, *outDir)
	if err != nil {
		fmt.Fprintf(os.Stderr, "warn: could not save report: %v\n", err)
		return
	}
	fmt.Printf("results saved: %s\n", outFile)
}

func resolveYoyo(flag string) string {
	if flag != "" {
		return flag
	}
	// prefer the local dev binary
	candidates := []string{
		os.Getenv("HOME") + "/.local/bin/yoyo",
		"/opt/homebrew/bin/yoyo",
		"yoyo",
	}
	for _, c := range candidates {
		if _, err := os.Stat(c); err == nil {
			return c
		}
	}
	return "yoyo"
}

func filterTasks(tasks []Task, id string) []Task {
	var out []Task
	for _, t := range tasks {
		if strings.EqualFold(t.ID, id) {
			out = append(out, t)
		}
	}
	return out
}
