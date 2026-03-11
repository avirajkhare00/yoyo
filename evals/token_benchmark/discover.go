package main

import (
	"encoding/json"
	"fmt"
	"os/exec"
	"regexp"
	"strings"
)

var fnNameRe = regexp.MustCompile(`(?:function|pub fn|func)\s+([a-zA-Z_][a-zA-Z0-9_]+)`)

// extractFnName pulls the function name out of a grep match line.
func extractFnName(line string) string {
	m := fnNameRe.FindStringSubmatch(line)
	if len(m) >= 2 {
		return m[1]
	}
	return ""
}

// RepoProfile holds real names discovered from the target repo at runtime.
type RepoProfile struct {
	// top complex function (for call-chain, function-body, caller-discovery)
	TopFn string
	// second most complex (for blast-radius — different from TopFn)
	SecondFn string
	// a large file (for function-body linux side)
	LargeFile string
	// module/package path (for module-deep-dive)
	TopModule string
	// a file with many functions (for file-overview)
	DensestFile string
}

type shakeResult struct {
	TopFunctions []struct {
		Name string `json:"name"`
		File string `json:"file"`
	} `json:"top_functions"`
}

type fileFunctionsResult struct {
	Functions []struct {
		Name string `json:"name"`
	} `json:"functions"`
}

type packageSummaryResult struct {
	Packages []struct {
		Path string `json:"path"`
	} `json:"packages"`
}

// discoverRepo queries the target repo to extract real function/file names.
func discoverRepo(yoyoBin, repoPath string) (RepoProfile, error) {
	profile := RepoProfile{}

	// ── shake: top complex functions ─────────────────────────────────────────
	shakeOut, err := exec.Command("sh", "-c",
		fmt.Sprintf("%s shake --path %s", yoyoBin, repoPath),
	).Output()
	if err != nil {
		return profile, fmt.Errorf("shake: %w", err)
	}

	var shake shakeResult
	if err := json.Unmarshal(shakeOut, &shake); err == nil && len(shake.TopFunctions) >= 2 {
		profile.TopFn = shake.TopFunctions[0].Name
		profile.SecondFn = shake.TopFunctions[1].Name
		profile.LargeFile = shake.TopFunctions[0].File
		// find densest file: pick file that appears most in top functions
		fileCounts := map[string]int{}
		for _, f := range shake.TopFunctions {
			fileCounts[f.File]++
		}
		best, bestN := "", 0
		for f, n := range fileCounts {
			if n > bestN {
				best, bestN = f, n
			}
		}
		profile.DensestFile = best
	}

	// ── package-summary: top module ───────────────────────────────────────────
	pkgOut, err := exec.Command("sh", "-c",
		fmt.Sprintf("%s package-summary --path %s", yoyoBin, repoPath),
	).Output()
	if err == nil {
		// parse first package path
		lines := strings.Split(string(pkgOut), "\n")
		for _, l := range lines {
			if strings.Contains(l, `"path"`) {
				// extract value between quotes after colon
				parts := strings.SplitN(l, ":", 2)
				if len(parts) == 2 {
					val := strings.Trim(strings.TrimSpace(parts[1]), `",`)
					if val != "" {
						profile.TopModule = val
						break
					}
				}
			}
		}
	}

	// ── grep fallback: when bake/shake fails on very large repos ─────────────
	// bake can stack-overflow on repos with 80K+ files. Fall back to grep to
	// extract real function names so the harness can still run.
	if profile.TopFn == "" {
		grepOut, gerr := exec.Command("sh", "-c",
			fmt.Sprintf(
				"grep -rh --include='*.ts' --include='*.rs' --include='*.go' --include='*.zig' "+
					"-E '^export (async )?function [a-zA-Z_][a-zA-Z0-9_]+|^pub fn [a-zA-Z_][a-zA-Z0-9_]+|^func [a-zA-Z_][a-zA-Z0-9_]+' "+
					"%s 2>/dev/null | grep -v '//' | head -20",
				repoPath,
			),
		).Output()
		if gerr == nil {
			for _, line := range strings.Split(string(grepOut), "\n") {
				name := extractFnName(line)
				if name == "" || len(name) < 4 {
					continue
				}
				if profile.TopFn == "" {
					profile.TopFn = name
				} else if profile.SecondFn == "" {
					profile.SecondFn = name
					break
				}
			}
		}
		// find a large file as fallback
		if profile.LargeFile == "" {
			findOut, _ := exec.Command("sh", "-c",
				fmt.Sprintf("find %s/src %s/lib -name '*.ts' -o -name '*.rs' -o -name '*.go' 2>/dev/null | head -1", repoPath, repoPath),
			).Output()
			profile.LargeFile = strings.TrimSpace(strings.TrimPrefix(string(findOut), repoPath+"/"))
		}
	}

	if profile.TopFn == "" {
		return profile, fmt.Errorf("could not discover any functions in repo")
	}

	fmt.Printf("  discovered: topFn=%s  secondFn=%s  file=%s  module=%s\n",
		profile.TopFn, profile.SecondFn, profile.LargeFile, profile.TopModule)

	return profile, nil
}
