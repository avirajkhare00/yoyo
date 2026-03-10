package main

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"time"
)

// BenchmarkReport is the full JSON output.
type BenchmarkReport struct {
	Timestamp string          `json:"timestamp"`
	RepoPath  string          `json:"repo_path"`
	Model     string          `json:"model"`
	Tasks     []TaskResult    `json:"tasks"`
	Summary   Summary         `json:"summary"`
}

type Summary struct {
	TotalTasks int `json:"total_tasks"`

	// token cost
	AvgLinuxTokens  int `json:"avg_linux_tokens"`
	AvgYoyoTokens   int `json:"avg_yoyo_tokens"`
	AvgReductionPct int `json:"avg_reduction_pct"`

	// latency
	AvgLinuxLatencyMs int64 `json:"avg_linux_latency_ms"`
	AvgYoyoLatencyMs  int64 `json:"avg_yoyo_latency_ms"`

	// tool calls
	AvgLinuxCalls int `json:"avg_linux_calls"`
	AvgYoyCalls   int `json:"avg_yoyo_calls"`

	// signal ratio
	AvgLinuxSignalPct int `json:"avg_linux_signal_pct"`
	AvgYoyoSignalPct  int `json:"avg_yoyo_signal_pct"`

	// hallucination
	TotalLinuxHalluc int `json:"total_linux_halluc"`
	TotalYoyoHalluc  int `json:"total_yoyo_halluc"`

	// LLM-as-judge
	AvgLinuxAccuracy     int `json:"avg_linux_accuracy"`
	AvgLinuxCompleteness int `json:"avg_linux_completeness"`
	AvgYoyoAccuracy      int `json:"avg_yoyo_accuracy"`
	AvgYoyoCompleteness  int `json:"avg_yoyo_completeness"`
}

func buildSummary(results []TaskResult) Summary {
	n := len(results)
	if n == 0 {
		return Summary{}
	}
	s := Summary{TotalTasks: n}
	for _, r := range results {
		s.AvgLinuxTokens += r.LinuxTokens
		s.AvgYoyoTokens += r.YoyoTokens
		s.AvgReductionPct += r.Reduction
		s.AvgLinuxLatencyMs += r.LinuxLatencyMs
		s.AvgYoyoLatencyMs += r.YoyoLatencyMs
		s.AvgLinuxCalls += r.LinuxCalls
		s.AvgYoyCalls += r.YoyCalls
		s.AvgLinuxSignalPct += r.LinuxSignalPct
		s.AvgYoyoSignalPct += r.YoyoSignalPct
		s.TotalLinuxHalluc += r.LinuxHalluc
		s.TotalYoyoHalluc += r.YoyoHalluc
		s.AvgLinuxAccuracy += r.LinuxAccuracy
		s.AvgLinuxCompleteness += r.LinuxCompleteness
		s.AvgYoyoAccuracy += r.YoyoAccuracy
		s.AvgYoyoCompleteness += r.YoyoCompleteness
	}
	_ = s // suppress unused warning before division
	s.AvgLinuxTokens /= n
	s.AvgYoyoTokens /= n
	s.AvgReductionPct /= n
	s.AvgLinuxLatencyMs /= int64(n)
	s.AvgYoyoLatencyMs /= int64(n)
	s.AvgLinuxCalls /= n
	s.AvgYoyCalls /= n
	s.AvgLinuxSignalPct /= n
	s.AvgYoyoSignalPct /= n
	s.AvgLinuxAccuracy /= n
	s.AvgLinuxCompleteness /= n
	s.AvgYoyoAccuracy /= n
	s.AvgYoyoCompleteness /= n
	return s
}

func printTable(results []TaskResult, summary Summary) {
	sep := strings.Repeat("-", 110)

	// header
	fmt.Println(sep)
	fmt.Printf("%-25s  %7s %7s %6s  %6s %6s  %5s %5s  %6s %6s  %4s %4s  %5s %5s\n",
		"Task",
		"LnTok", "YoTok", "Red%",
		"LnMs", "YoMs",
		"LnCl", "YoCl",
		"LnSig", "YoSig",
		"LnHl", "YoHl",
		"LnAcc", "YoAcc",
	)
	fmt.Println(sep)

	for _, r := range results {
		errNote := ""
		if r.Err != "" {
			errNote = " !"
		}
		fmt.Printf("%-25s  %7d %7d %5d%%  %6d %6d  %5d %5d  %5d%% %5d%%  %4d %4d  %5d %5d%s\n",
			r.TaskID,
			r.LinuxTokens, r.YoyoTokens, r.Reduction,
			r.LinuxLatencyMs, r.YoyoLatencyMs,
			r.LinuxCalls, r.YoyCalls,
			r.LinuxSignalPct, r.YoyoSignalPct,
			r.LinuxHalluc, r.YoyoHalluc,
			r.LinuxAccuracy, r.YoyoAccuracy,
			errNote,
		)
	}

	fmt.Println(sep)
	fmt.Printf("%-25s  %7d %7d %5d%%  %6d %6d  %5d %5d  %5d%% %5d%%  %4d %4d  %5d %5d\n",
		"AVERAGE",
		summary.AvgLinuxTokens, summary.AvgYoyoTokens, summary.AvgReductionPct,
		summary.AvgLinuxLatencyMs, summary.AvgYoyoLatencyMs,
		summary.AvgLinuxCalls, summary.AvgYoyCalls,
		summary.AvgLinuxSignalPct, summary.AvgYoyoSignalPct,
		summary.TotalLinuxHalluc, summary.TotalYoyoHalluc,
		summary.AvgLinuxAccuracy, summary.AvgYoyoAccuracy,
	)
	fmt.Println(sep)

	fmt.Printf("\nColumns: Tokens | Latency(ms) | ToolCalls | SignalRatio%% | Hallucinations | Accuracy/10 (LLM-judge)\n")
	fmt.Printf("\nSummary:\n")
	fmt.Printf("  Token reduction:     linux avg %d tok  →  yoyo avg %d tok  (%d%% reduction)\n",
		summary.AvgLinuxTokens, summary.AvgYoyoTokens, summary.AvgReductionPct)
	fmt.Printf("  Latency:             linux avg %dms  →  yoyo avg %dms\n",
		summary.AvgLinuxLatencyMs, summary.AvgYoyoLatencyMs)
	fmt.Printf("  Tool calls:          linux avg %.1f  →  yoyo avg %.1f\n",
		float64(summary.AvgLinuxCalls), float64(summary.AvgYoyCalls))
	fmt.Printf("  Signal ratio:        linux avg %d%%  →  yoyo avg %d%%\n",
		summary.AvgLinuxSignalPct, summary.AvgYoyoSignalPct)
	fmt.Printf("  Hallucinations:      linux total %d  →  yoyo total %d\n",
		summary.TotalLinuxHalluc, summary.TotalYoyoHalluc)
	fmt.Printf("  Accuracy (judge):    linux avg %d/10  →  yoyo avg %d/10\n",
		summary.AvgLinuxAccuracy, summary.AvgYoyoAccuracy)
	fmt.Printf("  Completeness:        linux avg %d/10  →  yoyo avg %d/10\n\n",
		summary.AvgLinuxCompleteness, summary.AvgYoyoCompleteness)
}

func saveReport(results []TaskResult, summary Summary, repoPath, outDir string) (string, error) {
	report := BenchmarkReport{
		Timestamp: time.Now().UTC().Format("2006-01-02T15:04:05Z"),
		RepoPath:  repoPath,
		Model:     "gpt-4o-mini",
		Tasks:     results,
		Summary:   summary,
	}
	ts := time.Now().Format("2006-01-02-150405")
	outFile := fmt.Sprintf("%s/token-benchmark-%s.json", outDir, ts)
	data, err := json.MarshalIndent(report, "", "  ")
	if err != nil {
		return "", err
	}
	if err := os.WriteFile(outFile, data, 0644); err != nil {
		return "", err
	}
	return outFile, nil
}
