package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os/exec"
	"regexp"
	"strings"
	"time"
)

const openAIURL = "https://api.openai.com/v1/chat/completions"

const systemPrompt = `You are a code assistant. Answer the question using ONLY the context provided.
Be concise and precise. If the context does not contain enough information, say so.`

const judgePrompt = `You are an eval judge scoring two answers to the same coding question.
Score each answer from 0-10 on:
- accuracy: is the answer factually correct given the question?
- completeness: does it fully answer all parts of the question?

Return ONLY valid JSON in this exact shape:
{"linux_accuracy":0,"linux_completeness":0,"yoyo_accuracy":0,"yoyo_completeness":0}`

// TaskResult holds the full outcome of running one task both ways.
type TaskResult struct {
	TaskID   string `json:"task_id"`
	Question string `json:"question"`

	// token cost
	LinuxTokens int `json:"linux_tokens"`
	YoyoTokens  int `json:"yoyo_tokens"`
	Reduction   int `json:"reduction_pct"`

	// latency (context gathering only, ms)
	LinuxLatencyMs int64 `json:"linux_latency_ms"`
	YoyoLatencyMs  int64 `json:"yoyo_latency_ms"`

	// tool call count
	LinuxCalls int `json:"linux_calls"`
	YoyCalls   int `json:"yoyo_calls"`

	// context signal ratio (answer tokens / context tokens, ×100 as int pct)
	LinuxSignalPct int `json:"linux_signal_pct"`
	YoyoSignalPct  int `json:"yoyo_signal_pct"`

	// hallucination (identifiers in answer not found in repo)
	LinuxHalluc int `json:"linux_halluc"`
	YoyoHalluc  int `json:"yoyo_halluc"`

	// LLM-as-judge scores (0–10)
	LinuxAccuracy     int `json:"linux_accuracy"`
	LinuxCompleteness int `json:"linux_completeness"`
	YoyoAccuracy      int `json:"yoyo_accuracy"`
	YoyoCompleteness  int `json:"yoyo_completeness"`

	// raw answers for inspection
	LinuxAnswer    string `json:"linux_answer"`
	YoyoAnswer     string `json:"yoyo_answer"`
	LinuxCtxChars  int    `json:"linux_ctx_chars"`
	YoyoCtxChars   int    `json:"yoyo_ctx_chars"`

	Err string `json:"error,omitempty"`
}

// ── OpenAI types ──────────────────────────────────────────────────────────────

type chatRequest struct {
	Model    string    `json:"model"`
	Messages []message `json:"messages"`
}

type message struct {
	Role    string `json:"role"`
	Content string `json:"content"`
}

type chatResponse struct {
	Choices []struct {
		Message message `json:"message"`
	} `json:"choices"`
	Usage struct {
		PromptTokens     int `json:"prompt_tokens"`
		CompletionTokens int `json:"completion_tokens"`
	} `json:"usage"`
	Error *struct {
		Message string `json:"message"`
	} `json:"error,omitempty"`
}

type judgeScores struct {
	LinuxAccuracy     int `json:"linux_accuracy"`
	LinuxCompleteness int `json:"linux_completeness"`
	YoyoAccuracy      int `json:"yoyo_accuracy"`
	YoyoCompleteness  int `json:"yoyo_completeness"`
}

// ── helpers ───────────────────────────────────────────────────────────────────

// runCmds executes shell commands and returns concatenated stdout + elapsed ms.
func runCmds(cmds []string, repoPath string) (string, int64, error) {
	start := time.Now()
	var sb strings.Builder
	for _, raw := range cmds {
		cmd := strings.ReplaceAll(raw, "{{REPO}}", repoPath)
		out, err := exec.Command("sh", "-c", cmd).Output()
		if err != nil {
			sb.WriteString(fmt.Sprintf("# cmd: %s\n# error: %v\n%s\n", cmd, err, string(out)))
			continue
		}
		sb.WriteString(fmt.Sprintf("# cmd: %s\n%s\n", cmd, string(out)))
	}
	return sb.String(), time.Since(start).Milliseconds(), nil
}

// callOpenAI sends a chat request and returns (content, promptTokens, error).
func callOpenAI(apiKey string, msgs []message) (string, int, error) {
	req := chatRequest{Model: "gpt-4o-mini", Messages: msgs}
	body, _ := json.Marshal(req)
	httpReq, _ := http.NewRequest("POST", openAIURL, bytes.NewReader(body))
	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set("Authorization", "Bearer "+apiKey)

	client := &http.Client{Timeout: 60 * time.Second}
	resp, err := client.Do(httpReq)
	if err != nil {
		return "", 0, fmt.Errorf("http: %w", err)
	}
	defer resp.Body.Close()

	raw, _ := io.ReadAll(resp.Body)
	var chatResp chatResponse
	if err := json.Unmarshal(raw, &chatResp); err != nil {
		return "", 0, fmt.Errorf("decode: %w", err)
	}
	if chatResp.Error != nil {
		return "", 0, fmt.Errorf("openai: %s", chatResp.Error.Message)
	}
	if len(chatResp.Choices) == 0 {
		return "", 0, fmt.Errorf("no choices")
	}
	return chatResp.Choices[0].Message.Content, chatResp.Usage.PromptTokens, nil
}

// askAnswer sends context + question to GPT and returns (answer, promptTokens).
func askAnswer(apiKey, context, question string) (string, int, error) {
	return callOpenAI(apiKey, []message{
		{Role: "system", Content: systemPrompt},
		{Role: "user", Content: fmt.Sprintf("Context:\n%s\n\nQuestion: %s", context, question)},
	})
}

// judgeAnswers asks GPT to score both answers 0–10 on accuracy + completeness.
func judgeAnswers(apiKey, question, linuxAnswer, yoyoAnswer string) (judgeScores, error) {
	prompt := fmt.Sprintf(
		"Question: %s\n\nLinux answer:\n%s\n\nYoyo answer:\n%s",
		question, linuxAnswer, yoyoAnswer,
	)
	content, _, err := callOpenAI(apiKey, []message{
		{Role: "system", Content: judgePrompt},
		{Role: "user", Content: prompt},
	})
	if err != nil {
		return judgeScores{}, err
	}
	// strip markdown fences if present
	content = strings.TrimSpace(content)
	content = strings.TrimPrefix(content, "```json")
	content = strings.TrimPrefix(content, "```")
	content = strings.TrimSuffix(content, "```")

	var scores judgeScores
	if err := json.Unmarshal([]byte(strings.TrimSpace(content)), &scores); err != nil {
		return judgeScores{}, fmt.Errorf("judge parse: %w (raw: %s)", err, content)
	}
	return scores, nil
}

// identRe matches backtick-quoted identifiers or snake_case words.
var identRe = regexp.MustCompile("`([a-zA-Z_][a-zA-Z0-9_]+)`")

// hallucinationCount checks how many quoted identifiers in answer don't exist in repo src/.
func hallucinationCount(answer, repoPath string) int {
	matches := identRe.FindAllStringSubmatch(answer, -1)
	count := 0
	for _, m := range matches {
		ident := m[1]
		if len(ident) < 4 {
			continue // skip short tokens
		}
		out, _ := exec.Command("sh", "-c",
			fmt.Sprintf("grep -r --include='*.rs' --include='*.go' -l %q %s/src 2>/dev/null | head -1", ident, repoPath),
		).Output()
		if strings.TrimSpace(string(out)) == "" {
			count++
		}
	}
	return count
}

// signalPct = answer_chars / context_chars * 100 (capped at 100).
func signalPct(answerLen, ctxLen int) int {
	if ctxLen == 0 {
		return 0
	}
	v := answerLen * 100 / ctxLen
	if v > 100 {
		return 100
	}
	return v
}

// ── main runner ───────────────────────────────────────────────────────────────

func runTask(t Task, repoPath, apiKey string) TaskResult {
	result := TaskResult{
		TaskID:     t.ID,
		Question:   t.Question,
		LinuxCalls: len(t.LinuxCmds),
		YoyCalls:   len(t.YoyoPlusCmds),
	}

	linuxCtx, linuxMs, _ := runCmds(t.LinuxCmds, repoPath)
	yoyoCtx, yoyoMs, _ := runCmds(t.YoyoPlusCmds, repoPath)

	result.LinuxCtxChars = len(linuxCtx)
	result.YoyoCtxChars = len(yoyoCtx)
	result.LinuxLatencyMs = linuxMs
	result.YoyoLatencyMs = yoyoMs

	linuxAnswer, linuxTokens, err := askAnswer(apiKey, linuxCtx, t.Question)
	if err != nil {
		result.Err = fmt.Sprintf("openai linux: %v", err)
		return result
	}

	yoyoAnswer, yoyoTokens, err := askAnswer(apiKey, yoyoCtx, t.Question)
	if err != nil {
		result.Err = fmt.Sprintf("openai yoyo: %v", err)
		return result
	}

	result.LinuxTokens = linuxTokens
	result.YoyoTokens = yoyoTokens
	if linuxTokens > 0 {
		result.Reduction = (linuxTokens - yoyoTokens) * 100 / linuxTokens
	}

	result.LinuxAnswer = linuxAnswer
	result.YoyoAnswer = yoyoAnswer

	result.LinuxSignalPct = signalPct(len(linuxAnswer), len(linuxCtx))
	result.YoyoSignalPct = signalPct(len(yoyoAnswer), len(yoyoCtx))

	result.LinuxHalluc = hallucinationCount(linuxAnswer, repoPath)
	result.YoyoHalluc = hallucinationCount(yoyoAnswer, repoPath)

	scores, err := judgeAnswers(apiKey, t.Question, linuxAnswer, yoyoAnswer)
	if err != nil {
		result.Err = fmt.Sprintf("judge: %v", err)
	} else {
		result.LinuxAccuracy = scores.LinuxAccuracy
		result.LinuxCompleteness = scores.LinuxCompleteness
		result.YoyoAccuracy = scores.YoyoAccuracy
		result.YoyoCompleteness = scores.YoyoCompleteness
	}

	return result
}
