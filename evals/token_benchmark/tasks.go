package main

import "fmt"

// Task defines one benchmark question answered two ways:
//   - LinuxCmds:     linux-only (grep, cat, find)
//   - YoyoPlusCmds:  yoyo tools + any linux helpers needed
//
// {{REPO}} in commands is replaced with the actual repo path at runtime.
type Task struct {
	ID           string
	Question     string
	LinuxCmds    []string // linux-only context gathering
	YoyoPlusCmds []string // yoyo tools + linux helpers combined
	GoldKeywords []string // all must appear in a correct answer (case-insensitive)
}

// buildTasks constructs repo-aware tasks using real function names from the profile.
func buildTasks(yoyoBin string, p RepoProfile) []Task {
	fn := p.TopFn
	fn2 := p.SecondFn
	file := p.LargeFile
	module := p.TopModule
	densestFile := p.DensestFile
	if densestFile == "" {
		densestFile = file
	}

	return []Task{
		// ── structural ───────────────────────────────────────────────────────────
		{
			ID: "caller-discovery",
			Question: fmt.Sprintf("Which functions call %s, and in which files?", fn),
			LinuxCmds: []string{
				fmt.Sprintf("grep -rn %s {{REPO}} --include='*.rs' --include='*.go'", fn),
			},
			YoyoPlusCmds: []string{
				fmt.Sprintf("%s blast-radius --path {{REPO}} --symbol %s", yoyoBin, fn),
				fmt.Sprintf("grep -rn %s {{REPO}} --include='*.rs' --include='*.go' -l", fn),
			},
			GoldKeywords: []string{fn},
		},
		{
			ID: "function-body",
			Question: fmt.Sprintf("What does the %s function do? Describe its logic in detail.", fn),
			LinuxCmds: []string{
				fmt.Sprintf("cat {{REPO}}/%s", file),
			},
			YoyoPlusCmds: []string{
				fmt.Sprintf("%s symbol --path {{REPO}} --name %s --include-source", yoyoBin, fn),
			},
			GoldKeywords: []string{fn},
		},
		{
			ID: "dead-code",
			Question: "Which functions are dead code (never called) in this project?",
			LinuxCmds: []string{
				"grep -rn 'pub fn\\|^func ' {{REPO}} --include='*.rs' --include='*.go' | head -80",
			},
			YoyoPlusCmds: []string{
				fmt.Sprintf("%s health --path {{REPO}}", yoyoBin),
			},
			GoldKeywords: []string{"dead"},
		},
		{
			ID: "call-chain",
			Question: fmt.Sprintf("What is the full call chain when %s is invoked?", fn2),
			LinuxCmds: []string{
				fmt.Sprintf("grep -rn %s {{REPO}} --include='*.rs' --include='*.go'", fn2),
				fmt.Sprintf("cat {{REPO}}/%s", file),
			},
			YoyoPlusCmds: []string{
				fmt.Sprintf("%s trace-down --path {{REPO}} --name %s", yoyoBin, fn2),
				fmt.Sprintf("grep -n 'fn %s\\|func %s' {{REPO}}/%s", fn2, fn2, file),
			},
			GoldKeywords: []string{fn2},
		},
		{
			ID: "large-functions",
			Question: "Which functions have the highest cyclomatic complexity in this project?",
			LinuxCmds: []string{
				fmt.Sprintf("cat {{REPO}}/%s", file),
				fmt.Sprintf("grep -rn 'pub fn\\|^func ' {{REPO}} --include='*.rs' --include='*.go' | head -60"),
			},
			YoyoPlusCmds: []string{
				fmt.Sprintf("%s shake --path {{REPO}}", yoyoBin),
			},
			GoldKeywords: []string{fn, "complexity"},
		},
		{
			ID: "suggest-placement",
			Question: "Where in the codebase should I add a new caching function? Which file and why?",
			LinuxCmds: []string{
				"find {{REPO}}/src -name '*.rs' -o -name '*.go' 2>/dev/null | head -30",
				fmt.Sprintf("grep -rn 'cache\\|Cache' {{REPO}} --include='*.rs' --include='*.go' -l | head -10"),
			},
			YoyoPlusCmds: []string{
				fmt.Sprintf("%s suggest-placement --path {{REPO}} --function-name cache --function-type util", yoyoBin),
				fmt.Sprintf("grep -rn 'cache\\|Cache' {{REPO}} --include='*.rs' --include='*.go' -l | head -5"),
			},
			GoldKeywords: []string{"cache"},
		},

		// ── semantic ─────────────────────────────────────────────────────────────
		{
			ID: "semantic-error-handling",
			Question: "Which functions handle errors or perform error recovery in this codebase?",
			LinuxCmds: []string{
				"grep -rn 'Err\\|error\\|recover\\|retry' {{REPO}} --include='*.rs' --include='*.go' | head -60",
			},
			YoyoPlusCmds: []string{
				fmt.Sprintf("%s semantic-search --path {{REPO}} --query 'error handling and recovery'", yoyoBin),
				fmt.Sprintf("%s semantic-search --path {{REPO}} --query 'return error result propagation'", yoyoBin),
			},
			GoldKeywords: []string{"error"},
		},
		{
			ID: "semantic-file-write",
			Question: "Which functions write data to disk or files?",
			LinuxCmds: []string{
				"grep -rn 'write\\|write_all\\|fs::write\\|File::create\\|os.WriteFile\\|ioutil.WriteFile' {{REPO}} --include='*.rs' --include='*.go' | head -60",
			},
			YoyoPlusCmds: []string{
				fmt.Sprintf("%s semantic-search --path {{REPO}} --query 'write data to disk or file'", yoyoBin),
				fmt.Sprintf("%s semantic-search --path {{REPO}} --query 'persist or save to filesystem'", yoyoBin),
			},
			GoldKeywords: []string{"write"},
		},
		{
			ID: "semantic-intent",
			Question: fmt.Sprintf("Find functions related to %s. What do they do?", fn),
			LinuxCmds: []string{
				fmt.Sprintf("grep -rn %s {{REPO}} --include='*.rs' --include='*.go' | head -60", fn),
			},
			YoyoPlusCmds: []string{
				fmt.Sprintf("%s semantic-search --path {{REPO}} --query '%s'", yoyoBin, fn),
				fmt.Sprintf("%s symbol --path {{REPO}} --name %s --include-source", yoyoBin, fn),
			},
			GoldKeywords: []string{fn},
		},

		// ── file/module structure ─────────────────────────────────────────────
		{
			ID: "file-overview",
			Question: fmt.Sprintf("What functions are defined in %s and what is their complexity?", densestFile),
			LinuxCmds: []string{
				fmt.Sprintf("grep -n 'pub fn\\|^func ' {{REPO}}/%s", densestFile),
			},
			YoyoPlusCmds: []string{
				fmt.Sprintf("%s file-functions --path {{REPO}} --file %s", yoyoBin, densestFile),
			},
			GoldKeywords: []string{fn},
		},
		{
			ID: "module-deep-dive",
			Question: fmt.Sprintf("What functions and responsibilities are in the %s module?", module),
			LinuxCmds: []string{
				fmt.Sprintf("find {{REPO}}/%s -name '*.rs' -o -name '*.go' 2>/dev/null | xargs cat 2>/dev/null | head -200", module),
			},
			YoyoPlusCmds: []string{
				fmt.Sprintf("%s package-summary --path {{REPO}} --package %s", yoyoBin, module),
			},
			GoldKeywords: []string{fn},
		},
		{
			ID: "architecture-overview",
			Question: "What is the overall structure of this codebase? Which modules handle what?",
			LinuxCmds: []string{
				"find {{REPO}}/src {{REPO}}/crates -name '*.rs' -o -name '*.go' 2>/dev/null | sort | head -40",
				"cat {{REPO}}/README.md 2>/dev/null | head -60",
			},
			YoyoPlusCmds: []string{
				fmt.Sprintf("%s architecture-map --path {{REPO}}", yoyoBin),
			},
			GoldKeywords: []string{"src"},
		},
	}
}
