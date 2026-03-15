use anyhow::Result;

use super::config::{load_yoyo_project_config, ProjectConventions};
use super::types::{
    build_compact_section, parse_section_cursor, BakeSummary, DecisionEntry, EndpointSummary,
    FunctionSummary, LlmWorkflowsCompactPayload, LlmWorkflowsPayload, LlmWorkflowsQueryPayload,
    Metapattern, MetapatternStep, ResponseView, ShakePayload, ToolDescription, Workflow,
    WorkflowQueryMatch, WorkflowStep, DEFAULT_COMPACT_LIMIT,
};
use super::util::{
    build_bake_index, load_bake_index, prepare_bake_artifacts_dir, project_snapshot,
    resolve_project_root,
};

/// Public entrypoint for the `llm_instructions` CLI/MCP tool.
pub fn llm_instructions(path: Option<String>) -> Result<String> {
    let root = resolve_project_root(path)?;
    let snapshot = project_snapshot(&root)?;
    let config = load_yoyo_project_config(&root)?;

    let catalog = tool_catalog();
    let mut groups: std::collections::BTreeMap<&str, Vec<&str>> = std::collections::BTreeMap::new();
    for t in &catalog {
        groups.entry(t.category).or_default().push(t.name);
    }

    let payload = serde_json::json!({
        "tool": "boot",
        "version": env!("CARGO_PKG_VERSION"),
        "project_root": root,
        "languages": snapshot.languages.into_iter().collect::<Vec<_>>(),
        "files_indexed": snapshot.files_indexed,
        "scopes": snapshot.scopes,
        "scope_dependencies": snapshot.scope_dependencies,
        "scoping_hints": snapshot.scoping_hints,
        "project_conventions": project_conventions_payload(config.as_ref().map(|cfg| &cfg.conventions)),
        "runtime_access": runtime_access_hint(),
        "user_config_files": user_config_files(),
        "managed_paths": managed_paths(),
        "tools": groups,
        "capabilities": capability_catalog(),
        "common_tasks": common_task_catalog(),
        "rules": [
            "Call index first and wait before any read-indexed tool.",
            "boot can be called in parallel with index on first contact.",
            "Read tools parallelise freely. Write tools are sequential.",
            "After any write, wait for completion before reading the same file.",
            "Call help(name) to get params, output shape, and examples for any tool or task topic.",
        ],
        "update_available": super::update::check_update(),
    });

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

fn capability_catalog() -> Vec<serde_json::Value> {
    use serde_json::json;

    vec![
        json!({
            "name": "orient",
            "question": "What kind of codebase is this?",
            "tools": ["boot", "index", "map", "routes", "health"],
        }),
        json!({
            "name": "locate",
            "question": "Where is the exact code object I need?",
            "tools": ["inspect", "search", "ask"],
        }),
        json!({
            "name": "judge",
            "question": "Where should this fix live and what must stay true?",
            "tools": ["judge_change", "inspect"],
        }),
        json!({
            "name": "relate",
            "question": "What else touches this and what breaks if I change it?",
            "tools": ["impact", "routes", "health"],
        }),
        json!({
            "name": "change",
            "question": "How do I make the change safely?",
            "tools": ["change"],
        }),
        json!({
            "name": "compose",
            "question": "When should I chain multiple yoyo reads into one result?",
            "tools": ["script"],
        }),
    ]
}

fn common_task_catalog() -> Vec<serde_json::Value> {
    use serde_json::json;

    vec![
        json!({
            "task": "Understand a new repo fast",
            "use": ["boot", "index", "map", "routes", "health"],
        }),
        json!({
            "task": "Find a function by name and read it",
            "use": ["inspect"],
        }),
        json!({
            "task": "Inspect a file or exact line range",
            "use": ["inspect"],
        }),
        json!({
            "task": "Find a function by intent",
            "use": ["ask", "inspect"],
        }),
        json!({
            "task": "Trace request path to business logic",
            "use": ["impact", "inspect"],
        }),
        json!({
            "task": "Judge ownership, invariants, and regression risk before editing",
            "use": ["judge_change", "inspect"],
        }),
        json!({
            "task": "Check whether a function is safe to delete",
            "use": ["impact", "health", "change"],
        }),
        json!({
            "task": "Rename or move code safely",
            "use": ["impact", "change"],
        }),
        json!({
            "task": "Patch multiple files in one change",
            "use": ["impact", "change"],
        }),
    ]
}

fn project_conventions_payload(conventions: Option<&ProjectConventions>) -> serde_json::Value {
    match conventions {
        Some(conventions) if !conventions.is_empty() => serde_json::json!({
            "configured": true,
            "source": "yoyo.json",
            "languages": conventions.languages,
            "frameworks": conventions.frameworks,
            "style_rules": conventions.style_rules,
            "commands": conventions.commands,
        }),
        _ => serde_json::json!({
            "configured": false,
            "source": "yoyo.json",
            "languages": [],
            "frameworks": [],
            "style_rules": [],
            "commands": {},
        }),
    }
}

fn user_config_files() -> Vec<serde_json::Value> {
    use serde_json::json;

    vec![json!({
        "path": "yoyo.json",
        "kind": "runtime_policy",
        "editable": true,
        "created_on_demand": true,
        "git_trackable": true,
        "agent_managed": true,
        "description": "Repo-root runtime execution policy for guarded writes. Agents can update this file to widen sandbox_prefix or allow_unsandboxed.",
    })]
}

fn runtime_access_hint() -> serde_json::Value {
    serde_json::json!({
        "config_path": "yoyo.json",
        "summary": "Agents can update yoyo.json to widen runtime execution for guarded writes.",
        "default": "least_privilege",
        "git_trackable": true,
        "agent_managed": true,
        "recommended_action": {
            "tool": "change",
            "action": "edit",
            "file": "yoyo.json"
        },
        "enable_unsandboxed_example": {
            "runtime": {
                "checks": [
                    {
                        "language": "python",
                        "command": ["python3", "{{file}}"],
                        "allow_unsandboxed": true,
                        "kind": "python-runtime",
                        "timeout_ms": 1000
                    }
                ]
            }
        },
        "limits": [
            "Commands must target the changed file with {{file}} or {{abs_file}}.",
            "Inline eval forms like python -c, node -e, and clojure -e are rejected."
        ]
    })
}

fn managed_paths() -> Vec<serde_json::Value> {
    use serde_json::json;

    vec![json!({
        "path": ".bakes/",
        "kind": "cache",
        "editable": false,
        "description": "Managed bake cache. Do not edit this path manually.",
    })]
}

/// Public entrypoint for the `llm_workflows` CLI/MCP tool.
/// Returns the full reference catalog: workflows, decision map, antipatterns, metapatterns.
/// Call on demand — not required for basic tool use.
pub fn llm_workflows(
    path: Option<String>,
    view: Option<String>,
    limit: Option<usize>,
    cursor: Option<String>,
    query: Option<String>,
) -> Result<String> {
    let _ = path; // unused; kept for API symmetry with llm_instructions
    let view = ResponseView::parse(view.as_deref())?;
    let compact_limit = limit.unwrap_or(DEFAULT_COMPACT_LIMIT);
    let cursor = parse_section_cursor(cursor.as_deref())?;
    let payload = llm_workflows_payload();

    if let Some(q) = query {
        return render_llm_workflows_query(&payload, q);
    }

    if matches!(view, ResponseView::Compact) {
        return render_llm_workflows_compact(&payload, view, compact_limit, cursor);
    }

    Ok(serde_json::to_string_pretty(&payload)?)
}

fn llm_workflows_payload() -> LlmWorkflowsPayload {
    LlmWorkflowsPayload {
        tool: "llm_workflows",
        version: env!("CARGO_PKG_VERSION"),
        workflows: workflow_catalog(),
        decision_map: decision_map(),
        antipatterns: llm_workflow_antipatterns(),
        metapatterns: metapattern_catalog(),
    }
}

fn llm_workflow_antipatterns() -> Vec<&'static str> {
    vec![
        "grep to count callers: overcounts — hits comments, docs, string literals, partial names. Use impact(symbol=...).",
        "grep to find a definition: returns all files containing the string, not the canonical definition. Use inspect(name=...).",
        "reading raw source to determine visibility: pub/pub(crate)/nothing requires inference and is error-prone. Use inspect(name=...) — visibility is structured.",
        "inferring module path from file path: conventions vary by language and project. Use inspect(name=...) — module_path is authoritative.",
        "str.replace to rename: corrupts partial matches (e.g. renaming is_match also renames is_match_candidate). Use change(action=rename).",
        "deleting a function without checking callers: leaves the codebase broken. Use impact(symbol=...) first, then change(action=delete).",
        "grep to list methods of a struct: returns all fn definitions in the file, not grouped by type. Use inspect(file=...) and filter the outlined functions by parent_type.",
        "grep to find trait implementors: matches impl blocks loosely, misses generic impls. Use inspect(name=...) — implementors are structured on trait matches.",
        "reading struct source to get field types: works but is unstructured. Use inspect(name=..., include_source=true) — fields stay parsed and typed.",
        "using raw editor-style writes to modify a function body: line numbers drift after edits, no reindex, no syntax check. Use change(action=edit) so the write resolves through the index and auto-reindexes.",
        "scraping human-oriented write rejection text to decide what to retry: brittle and lossy. Use the structured guard_failure payload first (operation, phase, retryable, files, errors), then use next_hint only as fallback guidance.",
        "calling multiple tools sequentially and combining their outputs manually: use script when you need to loop over tool output, filter arrays, cross-reference categories, or reduce across N items. One script call replaces N round-trips and returns a single structured result.",
    ]
}

fn workflow_matches(payload: &LlmWorkflowsPayload, words: &[&str]) -> Vec<WorkflowQueryMatch> {
    let mut matches = Vec::new();
    for workflow in &payload.workflows {
        let text = format!(
            "{} {} {}",
            workflow.name,
            workflow.description,
            workflow
                .steps
                .iter()
                .map(|step| format!("{} {}", step.tool, step.hint))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let score = query_score(&text, words);
        if score > 0 {
            matches.push(WorkflowQueryMatch {
                kind: "workflow",
                score,
                item: serde_json::json!({
                    "name": workflow.name,
                    "description": workflow.description,
                    "steps": workflow.steps.iter().map(|step| serde_json::json!({
                        "tool": step.tool,
                        "hint": step.hint,
                    })).collect::<Vec<_>>(),
                }),
            });
        }
    }
    matches
}

fn decision_matches(payload: &LlmWorkflowsPayload, words: &[&str]) -> Vec<WorkflowQueryMatch> {
    let mut matches = Vec::new();
    for decision in &payload.decision_map {
        let text = format!(
            "{} {} {} {}",
            decision.question, decision.right_tool, decision.right_field, decision.wrong_because
        );
        let score = query_score(&text, words);
        if score > 0 {
            matches.push(WorkflowQueryMatch {
                kind: "decision",
                score,
                item: serde_json::json!({
                    "question": decision.question,
                    "right_tool": decision.right_tool,
                    "right_field": decision.right_field,
                }),
            });
        }
    }
    matches
}

fn antipattern_matches(payload: &LlmWorkflowsPayload, words: &[&str]) -> Vec<WorkflowQueryMatch> {
    let mut matches = Vec::new();
    for antipattern in &payload.antipatterns {
        let score = query_score(antipattern, words);
        if score > 0 {
            matches.push(WorkflowQueryMatch {
                kind: "antipattern",
                score,
                item: serde_json::json!({ "text": antipattern }),
            });
        }
    }
    matches
}

fn metapattern_matches(payload: &LlmWorkflowsPayload, words: &[&str]) -> Vec<WorkflowQueryMatch> {
    let mut matches = Vec::new();
    for metapattern in &payload.metapatterns {
        let text = format!("{} {}", metapattern.shape, metapattern.when);
        let score = query_score(&text, words);
        if score > 0 {
            matches.push(WorkflowQueryMatch {
                kind: "metapattern",
                score,
                item: serde_json::json!({
                    "shape": metapattern.shape,
                    "when": metapattern.when,
                }),
            });
        }
    }
    matches
}

fn render_llm_workflows_query(payload: &LlmWorkflowsPayload, query: String) -> Result<String> {
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut matches = workflow_matches(payload, &words);
    matches.extend(decision_matches(payload, &words));
    matches.extend(antipattern_matches(payload, &words));
    matches.extend(metapattern_matches(payload, &words));
    matches.sort_by(|a, b| b.score.cmp(&a.score));
    matches.truncate(10);

    let result = LlmWorkflowsQueryPayload {
        tool: payload.tool,
        version: payload.version,
        query,
        matches,
    };
    Ok(serde_json::to_string_pretty(&result)?)
}

fn compact_workflow_items(payload: &LlmWorkflowsPayload) -> Vec<serde_json::Value> {
    payload
        .workflows
        .iter()
        .map(|workflow| {
            serde_json::json!({
                "name": workflow.name,
                "description": workflow.description,
                "steps": workflow.steps.len(),
                "first_tool": workflow.steps.first().map(|step| step.tool),
            })
        })
        .collect()
}

fn compact_decision_items(payload: &LlmWorkflowsPayload) -> Vec<serde_json::Value> {
    payload
        .decision_map
        .iter()
        .map(|entry| {
            serde_json::json!({
                "question": entry.question,
                "right_tool": entry.right_tool,
                "right_field": entry.right_field,
            })
        })
        .collect()
}

fn compact_antipattern_items(payload: &LlmWorkflowsPayload) -> Vec<serde_json::Value> {
    payload
        .antipatterns
        .iter()
        .map(|entry| serde_json::json!({ "text": entry }))
        .collect()
}

fn compact_metapattern_items(payload: &LlmWorkflowsPayload) -> Vec<serde_json::Value> {
    payload
        .metapatterns
        .iter()
        .map(|entry| {
            serde_json::json!({
                "shape": entry.shape,
                "when": entry.when,
                "instances": entry.instances.len(),
            })
        })
        .collect()
}

fn render_llm_workflows_compact(
    payload: &LlmWorkflowsPayload,
    view: ResponseView,
    compact_limit: usize,
    cursor: Option<(String, usize)>,
) -> Result<String> {
    let cursor_ref = cursor
        .as_ref()
        .map(|(section, offset)| (section.as_str(), *offset));
    let sections = vec![
        build_compact_section(
            "workflows",
            compact_workflow_items(payload),
            compact_limit,
            cursor_ref,
        ),
        build_compact_section(
            "decision_map",
            compact_decision_items(payload),
            compact_limit,
            cursor_ref,
        ),
        build_compact_section(
            "antipatterns",
            compact_antipattern_items(payload),
            compact_limit,
            cursor_ref,
        ),
        build_compact_section(
            "metapatterns",
            compact_metapattern_items(payload),
            compact_limit,
            cursor_ref,
        ),
    ]
    .into_iter()
    .flatten()
    .collect();

    let compact = LlmWorkflowsCompactPayload {
        tool: payload.tool,
        version: payload.version,
        view: view.as_str(),
        summary: format!(
            "{} workflows, {} decision entries, {} antipatterns, {} metapatterns",
            payload.workflows.len(),
            payload.decision_map.len(),
            payload.antipatterns.len(),
            payload.metapatterns.len(),
        ),
        sections,
        detail_hints: vec![
            "use --view raw for the full reference catalog",
            "use --cursor <section>:<offset> to page one section forward",
            "use llm_instructions first and call llm_workflows only when you need deeper guidance",
        ],
    };
    Ok(serde_json::to_string_pretty(&compact)?)
}

/// Score a text against a set of query words (case-insensitive word overlap).
/// Score a text against a set of query words (case-insensitive word overlap).
/// Stop words (how, do, i, a, an, the, to, in, of, for, is, it, be, with) are skipped.
fn query_score(text: &str, words: &[&str]) -> usize {
    const STOP: &[&str] = &[
        "how", "do", "i", "a", "an", "the", "to", "in", "of", "for", "is", "it", "be", "with",
        "what", "when", "where", "why", "can", "use", "get", "my",
    ];
    let lower = text.to_lowercase();
    words
        .iter()
        .filter(|w| !STOP.contains(&w.to_lowercase().as_str()))
        .filter(|w| lower.contains(&w.to_lowercase()))
        .count()
}

fn decision_map() -> Vec<DecisionEntry> {
    vec![
        DecisionEntry {
            question: "Where is function/struct/enum/trait X defined?",
            wrong_tool: "grep 'fn X' or 'struct X'",
            wrong_because: "Returns every file containing the string — comments, tests, re-exports, partials. 21 hits when answer is 1.",
            right_tool: "inspect",
            right_field: "file + start_line",
        },
        DecisionEntry {
            question: "Is X public, private, or crate-visible?",
            wrong_tool: "read raw source and infer from pub/pub(crate)/nothing",
            wrong_because: "Inference is error-prone and inconsistent across languages.",
            right_tool: "inspect",
            right_field: "visibility (exact enum: public | module | private)",
        },
        DecisionEntry {
            question: "What module or package does X belong to?",
            wrong_tool: "infer from file path",
            wrong_because: "Path conventions vary. For Rust, `src/` is stripped and crate name is inferred from workspace layout. mod re-exports break naive path inference entirely.",
            right_tool: "inspect",
            right_field: "module_path (e.g. tokio::sync, not tokio::src::sync)",
        },
        DecisionEntry {
            question: "What functions does X call?",
            wrong_tool: "grep for names inside the function body",
            wrong_because: "Cannot isolate calls *by* a specific function. Returns all occurrences in the file.",
            right_tool: "inspect",
            right_field: "calls[] (project-defined callees only, stdlib filtered)",
        },
        DecisionEntry {
            question: "Who calls X? How many callers?",
            wrong_tool: "grep for X and count lines",
            wrong_because: "Overcounts — hits comments, docs, string literals, partial names. 244 grep hits vs 29 real callers in tokio.",
            right_tool: "impact",
            right_field: "callers[] (deduplicated, non-self, no false positives)",
        },
        DecisionEntry {
            question: "What methods does struct X have?",
            wrong_tool: "grep 'fn' in the struct's file",
            wrong_because: "Returns all functions in the file with no grouping by impl block.",
            right_tool: "inspect",
            right_field: "functions[] filtered by parent_type == X",
        },
        DecisionEntry {
            question: "What fields does struct X have?",
            wrong_tool: "read struct source body",
            wrong_because: "Works but returns unstructured text — field types not queryable.",
            right_tool: "inspect with include_source=true",
            right_field: "fields[{name, type_str, visibility}] (Rust only)",
        },
        DecisionEntry {
            question: "What traits does struct X implement?",
            wrong_tool: "grep 'impl.*X'",
            wrong_because: "Matches loosely — hits impl blocks for other types, misses generic impls.",
            right_tool: "inspect",
            right_field: "implements[] on struct/enum matches",
        },
        DecisionEntry {
            question: "Which types implement trait X?",
            wrong_tool: "grep 'impl X for'",
            wrong_because: "Misses blanket impls, generic impls, re-exports. Requires manual deduplication.",
            right_tool: "inspect",
            right_field: "implementors[] on trait matches (deduplicated)",
        },
        DecisionEntry {
            question: "Which function is most complex / hardest to maintain?",
            wrong_tool: "none — no text tool can answer this",
            wrong_because: "Complexity requires parsing AST and counting branches across the whole codebase.",
            right_tool: "health",
            right_field: "large_functions[{name, file, score}]",
        },
        DecisionEntry {
            question: "What code is unused / dead?",
            wrong_tool: "none — no text tool can answer this",
            wrong_because: "Dead code detection requires a full call graph, not string search.",
            right_tool: "health",
            right_field: "dead_code[]",
        },
        DecisionEntry {
            question: "Rename X everywhere safely",
            wrong_tool: "str.replace / sed",
            wrong_because: "Corrupts partial matches — renaming is_match also renames is_match_candidate, is_match_at.",
            right_tool: "graph_rename",
            right_field: "word-boundary safe, scope-aware (file | package | project), atomic",
        },
        DecisionEntry {
            question: "Is it safe to delete X?",
            wrong_tool: "just delete it",
            wrong_because: "Leaves callers broken with no warning. spawn_blocking has 31 callers in tokio.",
            right_tool: "graph_delete",
            right_field: "blocks with caller list if callers exist; proceeds only when dead",
        },
        DecisionEntry {
            question: "Edit / patch function X by name",
            wrong_tool: "grep for line number, then Edit at that line",
            wrong_because: "Requires two tool calls. Line numbers drift after edits — stale lookups corrupt the wrong region.",
            right_tool: "patch with name= parameter",
            right_field: "resolves location from index — one call, immune to line drift",
        },
        DecisionEntry {
            question: "Find a function by what it does, not its name",
            wrong_tool: "grep for keywords or read many files",
            wrong_because: "No structural awareness. Returns every file containing the string, including comments, docs, tests. Cannot rank by relevance.",
            right_tool: "semantic_search",
            right_field: "results[{name, file, start_line, score}] — cosine similarity via local ONNX embeddings (score 0–1); TF-IDF fallback if DB absent",
        },
    ]
}

pub fn tool_catalog() -> Vec<ToolDescription> {
    vec![
        ToolDescription { name: "boot", description: "Bootstrap: tool names, categories, and concurrency rules. Call in parallel with index on first contact.", requires_bake: false, category: "bootstrap", parallelisable: false, output_shape: None },
        ToolDescription { name: "index", description: "Build the AST index all read-indexed tools depend on. Call in parallel with boot on first contact.", requires_bake: false, category: "bootstrap", parallelisable: false, output_shape: None },
        ToolDescription { name: "inspect", description: "Inspect a symbol, type, file outline, or line range from one entrypoint.", requires_bake: true, category: "read-indexed", parallelisable: true,
            output_shape: Some(r#"{"mode":"symbol|type|file|lines","target":{},"result":{}}"#) },
        ToolDescription { name: "map", description: "Directory tree with inferred roles.", requires_bake: true, category: "read-indexed", parallelisable: true, output_shape: None },
        ToolDescription { name: "search", description: "AST-aware search. Replaces grep/rg.", requires_bake: true, category: "read-indexed", parallelisable: true, output_shape: None },
        ToolDescription { name: "ask", description: "Find functions by intent using embeddings.", requires_bake: true, category: "read-indexed", parallelisable: true, output_shape: None },
        ToolDescription { name: "routes", description: "All detected HTTP routes.", requires_bake: true, category: "read-indexed", parallelisable: true, output_shape: None },
        ToolDescription { name: "judge_change", description: "Judge the likely ownership layer, candidate symbols, invariants, regression risks, and verification plan for a change.", requires_bake: true, category: "read-indexed", parallelisable: true,
            output_shape: Some(r#"{"ownership_layer":{"name":"","why":"","evidence_files":[]},"candidate_symbols":[{"name":"","file":"","start_line":0,"kind":"function|type","score":0.0,"why":"","incoming_callers":0,"caller_files":0}],"candidate_files":[{"file":"","role":"","why":""}],"invariants":[{"text":"","evidence":[]}],"regression_risks":[{"text":"","evidence":[]}],"verification_commands":[{"command":"","why":""}]}"#) },
        ToolDescription { name: "impact", description: "Task-shaped impact analysis for a symbol or endpoint.", requires_bake: true, category: "read-indexed", parallelisable: true,
            output_shape: Some(r#"{"mode":"symbol|endpoint","target":{},"summary":"(endpoint mode)","callers":[],"affected_files":[],"call_chain":[]}"#) },
        ToolDescription { name: "health", description: "Dead code, large functions, and duplicate hints.", requires_bake: true, category: "read-indexed", parallelisable: true,
            output_shape: Some(r#"{"large_functions":[{"name":"","file":"","start_line":0,"score":0,"complexity":0,"fan_out":0}],"dead_code":[{"name":"","file":"","start_line":0,"lines":0}],"duplicate_code":[{"stem":"","functions":[{"name":"","file":""}]}],"long_methods":[{"name":"","file":"","start_line":0,"lines":0}],"feature_envy":[{"name":"","file":"","envies":"","cross_file_calls":0}],"shotgun_surgery":[{"name":"","file":"","caller_files":0}]}"#) },
        ToolDescription { name: "change", description: "Task-shaped write entrypoint over edit, bulk_edit, rename, move, delete, create, and add.", requires_bake: false, category: "write", parallelisable: false,
            output_shape: Some(r#"{"action":"edit|bulk_edit|rename|move|delete|create|add","result":{}}"#) },
        ToolDescription { name: "retry_plan", description: "Turn a failed guarded write into a bounded retry plan with targeted inspect context.", requires_bake: false, category: "orchestration", parallelisable: true,
            output_shape: Some(r#"{"retryable":true,"max_retries":2,"guard_failure":{},"targets":[{"file":"","start_line":0,"end_line":0,"errors":[]}],"workflow":[{"id":"","tool":"","args":{}}]}"#) },
        ToolDescription { name: "script", description: "Run a Rhai script with yoyo tools as functions.", requires_bake: false, category: "orchestration", parallelisable: false, output_shape: None },
        ToolDescription { name: "help", description: "Get params, output shape, example, and limitations for any tool.", requires_bake: false, category: "discovery", parallelisable: true, output_shape: None },
    ]
}

pub fn tool_help(name: String) -> Result<String> {
    let help_entries = tool_help_catalog();
    let normalized = name.trim().to_ascii_lowercase();
    let entry = help_entries.iter().find(|e| e.name == normalized);
    match entry {
        Some(e) => Ok(serde_json::to_string_pretty(&serde_json::json!({
            "tool": "help",
            "version": env!("CARGO_PKG_VERSION"),
            "name": e.name,
            "params": e.params,
            "output_shape": e.output_shape,
            "example": e.example,
            "limitations": e.limitations,
        }))?),
        None => {
            let tasks = task_help_catalog();
            if let Some(task) = tasks.iter().find(|t| {
                t.name == normalized || t.aliases.iter().any(|alias| *alias == normalized)
            }) {
                return Ok(serde_json::to_string_pretty(&serde_json::json!({
                    "tool": "help",
                    "version": env!("CARGO_PKG_VERSION"),
                    "task": task.name,
                    "question": task.question,
                    "use": task.use_tools,
                    "why": task.why,
                    "steps": task.steps,
                }))?);
            }
            let available: Vec<&str> = help_entries.iter().map(|e| e.name).collect();
            let task_names: Vec<&str> = tasks.iter().map(|t| t.name).collect();
            Err(anyhow::anyhow!(
                "Unknown help topic '{}'. Tools: {}. Tasks: {}",
                name,
                available.join(", "),
                task_names.join(", ")
            ))
        }
    }
}

struct ToolHelp {
    name: &'static str,
    params: serde_json::Value,
    output_shape: Option<&'static str>,
    example: serde_json::Value,
    limitations: &'static str,
}

struct TaskHelp {
    name: &'static str,
    aliases: &'static [&'static str],
    question: &'static str,
    use_tools: &'static [&'static str],
    why: &'static str,
    steps: &'static [&'static str],
}

fn tool_help_catalog() -> Vec<ToolHelp> {
    use serde_json::json;
    vec![
        ToolHelp {
            name: "boot",
            params: json!({"path": "Optional project directory"}),
            output_shape: None,
            example: json!({"call": "boot()", "returns": "tool names, categories, concurrency rules"}),
            limitations: "Call once per session. Pair with index.",
        },
        ToolHelp {
            name: "index",
            params: json!({"path": "Optional project directory"}),
            output_shape: None,
            example: json!({"call": "index()", "returns": "files_indexed, languages"}),
            limitations: "Must complete before any read-indexed tool. Re-run after large external changes.",
        },
        ToolHelp {
            name: "inspect",
            params: json!({"name": "optional - symbol or type name for symbol/type mode", "file": "optional - file path for file or lines mode", "start_line": "optional - start line for lines mode", "end_line": "optional - end line for lines mode", "include_source": "optional bool - include body in symbol mode", "signature_only": "optional bool - return declaration/signature text only in symbol mode", "type_only": "optional bool - return a type surface instead of generic symbol matches", "include_summaries": "optional bool - include summaries in file mode", "depth": "optional - file structure depth: 1, 2, or all", "limit": "optional - max symbol matches", "stdlib": "optional bool - include stdlib in symbol/type mode", "path": "optional"}),
            output_shape: Some(r#"{"mode":"symbol|type|file|lines","target":{},"result":{}}"#),
            example: json!({"call": {"name": "handle_request", "signature_only": true}}),
            limitations: "Modes: name => symbol, name+type_only => type surface, file => outline, file+start_line+end_line => line range. Symbol/type and file modes require index; line-range mode does not. include_source cannot be combined with signature_only or type_only.",
        },
        ToolHelp {
            name: "map",
            params: json!({"intent": "optional - e.g. 'user handler'", "limit": "optional - max dirs (default 100)", "path": "optional"}),
            output_shape: Some(r#"{"directories":[{"path":"","role":"","file_count":0}],"total_dirs":0}"#),
            example: json!({"call": {"intent": "auth service"}}),
            limitations: "Requires index. Sorts by file count descending.",
        },
        ToolHelp {
            name: "search",
            params: json!({"query": "required - search text", "context": "optional - all|strings|comments|identifiers", "pattern": "optional - all|call|assign|return", "exclude_tests": "optional bool", "file": "optional - path substring", "limit": "optional - max matches (default 200)", "path": "optional"}),
            output_shape: Some(r#"{"query":"","matches":[{"file":"","line":0,"snippet":"","kind":""}],"total":0}"#),
            example: json!({"call": {"query": "handle_request", "context": "identifiers", "pattern": "call"}}),
            limitations: "Requires index. Use context=identifiers+pattern=call for call-site search.",
        },
        ToolHelp {
            name: "ask",
            params: json!({"query": "required - natural language description", "limit": "optional - max results (default 10)", "file": "optional - path substring", "path": "optional"}),
            output_shape: Some(r#"{"results":[{"name":"","file":"","start_line":0,"score":0.0}]}"#),
            example: json!({"call": {"query": "validate user token"}}),
            limitations: "Requires index. Uses ONNX embeddings when available, falls back to TF-IDF.",
        },
        ToolHelp {
            name: "judge_change",
            params: json!({"query": "required - engineering question, issue text, or failing-test summary", "symbol": "optional - known symbol hint", "file": "optional - file path substring", "limit": "optional - max candidate symbols (default 3, max 5)", "path": "optional"}),
            output_shape: Some(r#"{"ownership_layer":{"name":"","why":"","evidence_files":[]},"candidate_symbols":[{"name":"","file":"","start_line":0,"kind":"function|type","score":0.0,"why":"","incoming_callers":0,"caller_files":0}],"candidate_files":[{"file":"","role":"","why":""}],"rejected_alternatives":[{"name":"","file":"","reason":""}],"invariants":[{"text":"","evidence":[]}],"regression_risks":[{"text":"","evidence":[]}],"verification_commands":[{"command":"","why":""}]}"#),
            example: json!({"call": {"query": "Global gitignore matching breaks when searching an absolute path", "file": "ignore"}}),
            limitations: "Requires index. First cut is read-only judgment, not a patch planner. Use it to replace long search/inspect/impact chains before editing, then route writes through change for the error-bounded patch.",
        },
        ToolHelp {
            name: "routes",
            params: json!({"path": "optional"}),
            output_shape: Some(r#"{"endpoints":[{"method":"","path":"","handler":"","file":"","line":0}]}"#),
            example: json!({"call": {}}),
            limitations: "Requires index. Supports Express, Actix-web, Rocket, Flask, FastAPI, gin, echo, net/http.",
        },
        ToolHelp {
            name: "impact",
            params: json!({"symbol": "optional - function name for symbol-impact mode", "endpoint": "optional - URL path substring for endpoint-impact mode", "method": "optional - HTTP method filter for endpoint mode", "depth": "optional - max caller/call-chain depth", "include_source": "optional bool - include handler source in endpoint mode", "path": "optional"}),
            output_shape: Some(r#"{"mode":"symbol|endpoint","target":{},"callers":[],"affected_files":[],"handler":{},"call_chain":[]}"#),
            example: json!({"call": {"symbol": "handle_request", "depth": 3}}),
            limitations: "Requires index. Exactly one of symbol or endpoint must be provided. Use symbol mode before rename/move/delete and endpoint mode before route-level edits.",
        },
        ToolHelp {
            name: "health",
            params: json!({"top": "optional - max per category (default 10)", "view": "optional - compact|full|raw", "limit": "optional - items per section in compact", "cursor": "optional - pagination cursor", "path": "optional"}),
            output_shape: Some(r#"{"dead_code":[],"large_functions":[],"duplicate_code":[],"long_methods":[],"feature_envy":[],"shotgun_surgery":[]}"#),
            example: json!({"call": {"top": 5}}),
            limitations: "Requires index. Router-registered handlers may appear dead; confirm with impact(symbol=...) before deleting API code.",
        },
        ToolHelp {
            name: "change",
            params: json!({"action": "required - edit|bulk_edit|rename|move|delete|create|add", "name": "optional - symbol name for edit/rename/move/delete/add", "file": "optional - file path", "start_line": "optional - line-range edit start", "end_line": "optional - line-range edit end", "new_content": "optional - replacement for edit", "old_string": "optional - exact content-match source", "new_string": "optional - content-match replacement", "match_index": "optional - disambiguate symbol edit", "edits": "optional - array of {file, byte_start, byte_end, new_content} for bulk_edit", "new_name": "optional - rename target", "to_file": "optional - move destination", "force": "optional bool - allow delete with callers", "function_name": "optional - create scaffold name", "entity_type": "optional - add scaffold type", "after_symbol": "optional - add insertion anchor", "language": "optional - scaffold language", "params": "optional - typed params JSON for create/add", "returns": "optional - return type for create/add", "on": "optional - receiver/owner type for add", "path": "optional"}),
            output_shape: Some(r#"{"action":"edit|bulk_edit|rename|move|delete|create|add","result":{}}"#),
            example: json!({"call": {"action": "rename", "name": "get_user", "new_name": "fetch_user"}}),
            limitations: "Task-shaped router over existing write primitives. Use when you know the change intent but do not want to choose between edit, rename, move, delete, create, add, or bulk_edit first.",
        },
        ToolHelp {
            name: "retry_plan",
            params: json!({"text": "required - failed write output containing a guard_failure line or raw guard_failure JSON", "path": "optional - project directory; falls back to project_root inside the payload", "max_retries": "optional - bounded retry budget (default 2)", "context_lines": "optional - surrounding lines to inspect around each failure (default 3)"}),
            output_shape: Some(r#"{"tool":"guard_retry_plan","retryable":true,"max_retries":2,"next_hint":"","guard_failure":{},"targets":[{"file":"","start_line":0,"end_line":0,"errors":[],"inspect":{}}],"workflow":[{"id":"","tool":"","args":{},"why":""}]}"#),
            example: json!({"call": {"text": "guard_failure: {\"tool\":\"guard_failure\",\"operation\":\"patch\",\"phase\":\"post_write_guard\",\"retryable\":true,\"files_restored\":true,\"files\":[{\"file\":\"src/lib.rs\",\"errors\":[{\"line\":2,\"kind\":\"rust\",\"text\":\"mismatched types\"}]}]}", "max_retries": 2}}),
            limitations: "Consumes a prior guarded-write failure; it does not repair code itself. Uses inspect line mode, so it can work without a bake index as long as the target files still exist.",
        },
        ToolHelp {
            name: "script",
            params: json!({"code": "required - Rhai script", "path": "optional"}),
            output_shape: Some(r#"{"tool":"script","result":"(last expression value)"}"#),
            example: json!({"call": {"code": "let j = judge_change(#{query: \"validate user token flow\"}); j[\"ownership_layer\"]"}}),
            limitations: "Task-shaped script surface only: boot, index, inspect, search, ask, map, routes, judge_change, impact, health, change, help. Prefer script when you need loops, filtering, or aggregation across task-tool results.",
        },
        ToolHelp {
            name: "help",
            params: json!({"name": "required - tool or task topic"}),
            output_shape: Some(r#"{"tool":"help","name":"","params":{},"example":{},"limitations":""}"#),
            example: json!({"call": {"name": "safe delete"}}),
            limitations: "Supports both tool names and task topics such as inspect code, safe delete, trace request, find by intent, assess impact, and multi-file patch.",
        },
    ]
}

fn task_help_catalog() -> Vec<TaskHelp> {
    vec![
        TaskHelp {
            name: "judge change",
            aliases: &["ownership analysis", "judge ownership", "invariants and risks"],
            question: "How do I identify the likely ownership layer, invariants, and regression risks before editing?",
            use_tools: &["judge_change", "inspect", "change"],
            why: "judge_change is the high-level read surface for grounded change triage: it returns candidate symbols/files, the likely ownership layer, invariants, regression risks, and verification commands in one call, while change remains the error-bounded write surface.",
            steps: &[
                "Start with judge_change(query=...) to get the likely ownership layer and top candidates.",
                "Use inspect(...) on one candidate only if the ownership layer or invariant list needs confirmation.",
                "Use change(action=...) after you accept the judged ownership layer and minimal verification plan.",
            ],
        },
        TaskHelp {
            name: "inspect code",
            aliases: &["inspect symbol", "inspect file", "inspect lines"],
            question: "How do I read code without choosing between symbol, outline, and line-range tools first?",
            use_tools: &["inspect"],
            why: "inspect is the merged read surface: name => symbol mode, file => file-outline mode, file+start_line+end_line => exact line-range mode.",
            steps: &[
                "Use inspect(name=...) when you know the symbol and want the source or metadata.",
                "Use inspect(name=..., signature_only=true) when you only need the API shape.",
                "Use inspect(name=..., type_only=true) when you want a type surface with fields and methods.",
                "Use inspect(file=..., depth=1|2|all) when you want a cheaper file outline.",
                "Use inspect(file=..., start_line=..., end_line=...) when you need exact lines.",
            ],
        },
        TaskHelp {
            name: "safe delete",
            aliases: &["delete safety", "is it safe to delete", "safe deletion"],
            question: "How do I confirm a function is really safe to remove?",
            use_tools: &["impact", "health", "change"],
            why: "Delete safety is about structural impact, not text search. impact(symbol=...) finds live edges, health surfaces dead-code candidates, and change(action=delete) routes to the blocking delete primitive.",
            steps: &[
                "Run impact(symbol=...) first to see whether anything still reaches the symbol.",
                "Use health as a second signal for dead-code candidates, especially when cleaning up groups of functions.",
                "Run change(action=delete) only after impact is clean; it will still block unless force=true.",
            ],
        },
        TaskHelp {
            name: "trace request",
            aliases: &["trace request path", "request flow", "trace endpoint"],
            question: "How do I follow an HTTP endpoint into business logic?",
            use_tools: &["impact", "inspect"],
            why: "impact(endpoint=...) returns the endpoint, handler, and downstream chain in one task-shaped call; inspect is the fastest way to inspect any node in that chain with source or exact lines.",
            steps: &[
                "Start with impact(endpoint=...) to identify the matched route, handler, and downstream chain.",
                "Use inspect(name=..., include_source=true) on the handler or any downstream callee that needs inspection.",
            ],
        },
        TaskHelp {
            name: "assess impact",
            aliases: &["impact analysis", "what breaks", "blast radius"],
            question: "How do I understand what else a symbol or endpoint touches before making a change?",
            use_tools: &["impact", "inspect", "change"],
            why: "impact is the merged relation surface: symbol mode scopes callers and affected files, endpoint mode traces the request path into code, and inspect/change handle follow-up reads and edits.",
            steps: &[
                "Use impact(symbol=...) before rename, move, or delete work.",
                "Use impact(endpoint=...) before changing handler logic for a route.",
                "Open the most relevant caller or handler with inspect(...) before making the change.",
            ],
        },
        TaskHelp {
            name: "find by intent",
            aliases: &["semantic search", "find function by intent", "find by behavior"],
            question: "How do I find code when I know behavior but not the function name?",
            use_tools: &["ask", "inspect"],
            why: "ask ranks functions by intent; inspect then verifies the winning candidate with exact source and metadata.",
            steps: &[
                "Use ask with a short behavior description such as 'validate user token' or 'spawn blocking task'.",
                "Open the top candidate with inspect(name=..., include_source=true) before editing or tracing further.",
            ],
        },
        TaskHelp {
            name: "multi-file patch",
            aliases: &["patch multiple files", "bulk patch", "cross-file fix"],
            question: "How should I change several files in one safe pass?",
            use_tools: &["impact", "change"],
            why: "impact helps define the affected path for either a symbol or an endpoint; change(action=bulk_edit) applies all edits in one indexed write instead of many fragile string replacements.",
            steps: &[
                "Use impact(...) or other task-shaped read tools first to bound the full set of affected files.",
                "Apply the final cross-file change with change(action=bulk_edit) so offsets stay coherent in one operation.",
            ],
        },
    ]
}

fn workflow_catalog() -> Vec<Workflow> {
    vec![
        Workflow {
            name: "First-time setup",
            description: "Index the project before using any bake-dependent tool.",
            steps: vec![
                WorkflowStep { tool: "bake",  hint: "Build the index (auto-refreshes on future source changes)" },
                WorkflowStep { tool: "shake", hint: "Get a high-level overview of the codebase" },
            ],
        },
        Workflow {
            name: "Explore a function",
            description: "Find a function by name and read its source.",
            steps: vec![
                WorkflowStep { tool: "supersearch", hint: "Search by name or pattern to find the function" },
                WorkflowStep { tool: "symbol",      hint: "Exact lookup; set include_source=true to get the body inline" },
                WorkflowStep { tool: "slice",       hint: "Read surrounding context using start_line/end_line from symbol" },
            ],
        },
        Workflow {
            name: "Add a new feature",
            description: "Decide where to place a new function and scaffold it.",
            steps: vec![
                WorkflowStep { tool: "architecture_map",  hint: "Understand directory roles; pass your intent (e.g. 'user handler')" },
                WorkflowStep { tool: "suggest_placement", hint: "Get ranked file suggestions for the new function" },
                WorkflowStep { tool: "graph_create",      hint: "If adding to a new file: create the file + initial scaffold in one call. Errors if file exists." },
                WorkflowStep { tool: "graph_add",         hint: "If adding to an existing file: insert a scaffold at the right location (optionally after_symbol); index auto-updates" },
                WorkflowStep { tool: "patch",             hint: "Fill in the scaffold body — use name mode (pass symbol name) or old_string/new_string mode" },
            ],
        },
        Workflow {
            name: "Understand an API endpoint",
            description: "Trace an HTTP route to its handler and full call chain in one call.",
            steps: vec![
                WorkflowStep { tool: "flow", hint: "Pass endpoint path substring (and optional method). Returns handler + call chain + boundaries in one call. Prefer over api_trace + trace_down + symbol." },
                WorkflowStep { tool: "all_endpoints", hint: "If flow returns no match, list all detected routes to find the right path substring" },
            ],
        },
        Workflow {
            name: "Impact analysis",
            description: "Find everything that will break if you change a function.",
            steps: vec![
                WorkflowStep { tool: "symbol",       hint: "Confirm the exact symbol name exists in the index" },
                WorkflowStep { tool: "blast_radius", hint: "Get all transitive callers and affected files" },
                WorkflowStep { tool: "symbol",       hint: "Inspect each caller for context" },
                WorkflowStep { tool: "slice",        hint: "Read caller bodies to understand the coupling" },
            ],
        },
        Workflow {
            name: "Deep-dive into a module",
            description: "Understand a package or directory end-to-end.",
            steps: vec![
                WorkflowStep { tool: "package_summary", hint: "Get all files, functions, and endpoints for a path substring" },
                WorkflowStep { tool: "file_functions",  hint: "List functions per file with complexity scores" },
                WorkflowStep { tool: "slice",           hint: "Read specific functions using their line ranges" },
            ],
        },
        Workflow {
            name: "Search for code patterns",
            description: "Find usages, assignments, or calls across the codebase.",
            steps: vec![
                WorkflowStep { tool: "supersearch", hint: "Use context=identifiers and pattern=call for call-site search" },
                WorkflowStep { tool: "slice",       hint: "Read matches in context using the returned line numbers" },
            ],
        },
        Workflow {
            name: "Find a function by intent (semantic search)",
            description: "You know what a function does but not its name. Use semantic_search to find ranked candidates.",
            steps: vec![
                WorkflowStep { tool: "semantic_search", hint: "Pass a natural-language query, e.g. 'validate user token' or 'spawn blocking task'. Returns cosine-similarity ranked matches (0–1 score). Requires bake to have run first to build the embeddings DB." },
                WorkflowStep { tool: "symbol",          hint: "Confirm the top match with include_source=true to read the body" },
            ],
        },
        Workflow {
            name: "Edit a function",
            description: "Read a function and replace its body.",
            steps: vec![
                WorkflowStep { tool: "symbol",           hint: "Fetch the current body with include_source=true" },
                WorkflowStep { tool: "patch",  hint: "Write the new body — pass name + new_content, or use old_string/new_string for content-match mode" },
            ],
        },
        Workflow {
            name: "Find configuration and docs",
            description: "Locate README, .env, config, or Dockerfile.",
            steps: vec![
                WorkflowStep { tool: "find_docs", hint: "Use doc_type: readme | env | config | docker | all" },
                WorkflowStep { tool: "slice",     hint: "Read the first N lines of any matched file" },
            ],
        },
        Workflow {
            name: "Graph rename (one-shot)",
            description: "Rename an identifier at its definition and every call site in one call. No multi-step setup required.",
            steps: vec![
                WorkflowStep { tool: "graph_rename", hint: "Pass name (old) and new_name; word-boundary matching prevents partial renames; index is auto-updated" },
                WorkflowStep { tool: "symbol",       hint: "Verify the definition now carries the new name" },
            ],
        },
        Workflow {
            name: "Add a function scaffold",
            description: "Insert a new empty function body at the right location, then fill it in.",
            steps: vec![
                WorkflowStep { tool: "graph_add",        hint: "Specify entity_type (fn/function/def/func), name, file, and optionally after_symbol" },
                WorkflowStep { tool: "patch",  hint: "Fill in the generated scaffold — use name mode or old_string/new_string" },
            ],
        },
        Workflow {
            name: "Move a function between files",
            description: "Relocate a function to a different module/file and keep the index consistent.",
            steps: vec![
                WorkflowStep { tool: "bake",       hint: "Ensure byte_start/byte_end offsets are fresh" },
                WorkflowStep { tool: "graph_move", hint: "Pass the function name and destination file; source removal and dest append happen atomically" },
            ],
        },
        Workflow {
            name: "Safely delete dead code",
            description: "Confirm a function is truly unused before removing it. The combination prevents broken builds.",
            steps: vec![
                WorkflowStep { tool: "health",       hint: "Get dead code candidates — functions with no detected callers" },
                WorkflowStep { tool: "impact", hint: "Cross-check the candidate before deletion; health can miss router-registered handlers" },
                WorkflowStep { tool: "change", hint: "Run change(action=delete) only after impact is clean; deletion still blocks unless forced" },
            ],
        },
        Workflow {
            name: "Fix a broken API endpoint end-to-end",
            description: "Trace a route to its full call chain and patch every affected layer in one session.",
            steps: vec![
                WorkflowStep { tool: "impact", hint: "Pass the endpoint path substring — returns handler + full call chain + boundaries in one call" },
                WorkflowStep { tool: "inspect", hint: "Read each function in the chain with include_source=true to understand the failure" },
                WorkflowStep { tool: "change", hint: "Apply the final fix with edit or bulk_edit once the affected files are confirmed" },
            ],
        },
        Workflow {
            name: "Rename with safety check",
            description: "Understand the blast radius before renaming, then rename atomically.",
            steps: vec![
                WorkflowStep { tool: "impact", hint: "Scope the impact — see all callers and affected files before touching anything" },
                WorkflowStep { tool: "change", hint: "Run change(action=rename) to rename at definition + every call site atomically" },
                WorkflowStep { tool: "inspect",       hint: "Verify the definition carries the new name" },
            ],
        },
        Workflow {
            name: "Orient to an unfamiliar codebase",
            description: "Build a mental model of a new project from the outside in.",
            steps: vec![
                WorkflowStep { tool: "shake",            hint: "Language breakdown, file count, top-complexity functions — 30-second overview" },
                WorkflowStep { tool: "architecture_map", hint: "Directory tree with inferred roles (routes, services, models, etc.)" },
                WorkflowStep { tool: "all_endpoints",    hint: "All HTTP routes — understand the API surface" },
                WorkflowStep { tool: "health",           hint: "Dead code and large functions — where is the rot?" },
            ],
        },
        Workflow {
            name: "Manual rename with bounded scope",
            description: "Use task-shaped tools to bound impact, inspect the exact sites, and then apply a targeted bulk edit when you intentionally do not want a global rename.",
            steps: vec![
                WorkflowStep { tool: "impact",  hint: "Find all callers and affected files before touching anything" },
                WorkflowStep { tool: "search",   hint: "Search for the old name with context=identifiers to collect the intended call sites" },
                WorkflowStep { tool: "change",   hint: "Use change(action=bulk_edit) if you intentionally want a bounded rename instead of a graph rename" },
            ],
        },
        Workflow {
            name: "Precise local edit",
            description: "Inspect the exact lines you want to touch, then apply a narrow line-range or symbol edit.",
            steps: vec![
                WorkflowStep { tool: "inspect",      hint: "Read the function or exact lines first so the change target is unambiguous" },
                WorkflowStep { tool: "change", hint: "Use change(action=edit) with line-range mode or symbol mode for the narrowest safe patch" },
            ],
        },
        Workflow {
            name: "Recover from guard failure",
            description: "When a write is rejected, use the structured guard_failure payload as the retry driver. Repair only the named files, retry a small bounded number of times, and use next_hint as fallback guidance rather than the source of truth.",
            steps: vec![
                WorkflowStep { tool: "change",  hint: "Attempt the write. If it fails, parse guard_failure first: operation, phase, retryable, files_restored, and per-file errors." },
                WorkflowStep { tool: "inspect", hint: "Read only the named files or symbols from guard_failure.files[*]; fix the concrete compiler/interpreter/runtime errors, not adjacent code." },
                WorkflowStep { tool: "change",  hint: "Retry the write up to 2-3 times when retryable=true. Stop on config/setup errors, missing symbols, unsafe runtime config, or unchanged repeated failures. Use next_hint only as secondary guidance." },
            ],
        },
        Workflow {
            name: "Trace a call chain",
            description: "Follow a function's callees downward to database, HTTP, or queue boundaries.",
            steps: vec![
                WorkflowStep { tool: "impact", hint: "Use symbol mode for upstream callers or endpoint mode for request-path tracing" },
                WorkflowStep { tool: "inspect",     hint: "Inspect any relevant caller, handler, or downstream callee with include_source=true" },
            ],
        },
        Workflow {
            name: "Cross-reference health smells (script)",
            description: "Find functions flagged across multiple health categories — e.g. large AND dead — in one script call. Use script when you need to loop, filter, or cross-reference tool outputs. Individual tool calls cannot do this without manual inspection.",
            steps: vec![
                WorkflowStep { tool: "health",  hint: "Returns dead_code, large_functions, shotgun_surgery, feature_envy, duplicate_code — each an array" },
                WorkflowStep { tool: "script",  hint: r#"let h = health(); let large = h["large_functions"]; let dead = h["dead_code"]; let large_names = large.map(|f| f["name"]); dead.filter(|d| large_names.contains(d["name"]))"# },
            ],
        },
        Workflow {
            name: "Batch blast-radius scan (script)",
            description: "Run impact(symbol=...) on every large function and collect which ones are safe to refactor (zero callers). Replaces N sequential impact calls.",
            steps: vec![
                WorkflowStep { tool: "health",  hint: "Get large_functions list" },
                WorkflowStep { tool: "script",  hint: r#"let h = health(); let results = []; for f in h["large_functions"] { let impact_result = impact(#{symbol: f["name"]}); let callers = impact_result["callers"]; results += [#{ name: f["name"], callers: callers.len(), safe: callers.len() == 0 }]; } results"# },
            ],
        },
        Workflow {
            name: "Triage dead code by visibility (script)",
            description: "Classify dead code into public (API surface risk) vs private (safe to delete). Filters health output with a loop — not possible with a single tool call.",
            steps: vec![
                WorkflowStep { tool: "script",  hint: r#"let h = health(); let pub_dead = []; let priv_dead = []; for d in h["dead_code"] { if d["visibility"] == "public" { pub_dead += [d["name"]]; } else { priv_dead += [d["name"]]; } } #{ public_dead: pub_dead, private_dead_count: priv_dead.len() }"# },
                WorkflowStep { tool: "impact", hint: "Cross-check any public_dead candidates before deleting — routers register handlers invisibly to the AST" },
            ],
        },
    ]
}

fn metapattern_catalog() -> Vec<Metapattern> {
    vec![
        Metapattern {
            shape: "Orient → Scope → Read",
            when: "You're unfamiliar with a codebase, a module, or a domain area. Build the mental model before touching anything.",
            steps: vec![
                MetapatternStep { phase: "Orient",  tools: vec!["boot", "map"] },
                MetapatternStep { phase: "Scope",   tools: vec!["routes", "health"] },
                MetapatternStep { phase: "Read",    tools: vec!["inspect"] },
            ],
            instances: vec!["Orient to an unfamiliar codebase", "Deep-dive into a module", "Find a function by intent (semantic search)"],
        },
        Metapattern {
            shape: "Read → Safety → Write → Verify",
            when: "You're about to mutate code. Never write blind — always read first, check blast radius, then patch.",
            steps: vec![
                MetapatternStep { phase: "Read",    tools: vec!["inspect"] },
                MetapatternStep { phase: "Safety",  tools: vec!["impact"] },
                MetapatternStep { phase: "Write",   tools: vec!["change"] },
                MetapatternStep { phase: "Verify",  tools: vec!["inspect"] },
            ],
            instances: vec!["Edit a function", "Rename with safety check", "Fix a broken API endpoint end-to-end"],
        },
        Metapattern {
            shape: "Suspect → Confirm → Remove",
            when: "You think something is dead weight. Surface candidates, confirm no hidden callers, then delete.",
            steps: vec![
                MetapatternStep { phase: "Suspect", tools: vec!["health"] },
                MetapatternStep { phase: "Confirm", tools: vec!["impact"] },
                MetapatternStep { phase: "Remove",  tools: vec!["change"] },
            ],
            instances: vec!["Safely delete dead code"],
        },
        Metapattern {
            shape: "Orient → Place → Scaffold → Implement",
            when: "You're adding new functionality. Find the right home first, scaffold the shape, then fill in the body.",
            steps: vec![
                MetapatternStep { phase: "Orient",    tools: vec!["architecture_map"] },
                MetapatternStep { phase: "Place",     tools: vec!["suggest_placement"] },
                MetapatternStep { phase: "Scaffold",  tools: vec!["graph_create", "graph_add"] },
                MetapatternStep { phase: "Implement", tools: vec!["patch"] },
            ],
            instances: vec!["Add a new feature", "Add a function scaffold"],
        },
        Metapattern {
            shape: "Trace → Read → Fix",
            when: "Something is broken. Follow the path from entry point to failure, read each layer, then patch the root cause.",
            steps: vec![
                MetapatternStep { phase: "Trace", tools: vec!["impact", "search"] },
                MetapatternStep { phase: "Read",  tools: vec!["inspect"] },
                MetapatternStep { phase: "Fix",   tools: vec!["change"] },
            ],
            instances: vec!["Fix a broken API endpoint end-to-end", "Trace a call chain", "Understand an API endpoint"],
        },
    ]
}

/// Public entrypoint for the `shake` (repository overview) tool.
pub fn shake(path: Option<String>) -> Result<String> {
    let root = resolve_project_root(path)?;

    if let Some(bake) = load_bake_index(&root)? {
        // Use rich data from the bake index when available.
        let mut top_functions: Vec<FunctionSummary> = bake
            .functions
            .iter()
            .map(|f| FunctionSummary {
                name: f.name.clone(),
                file: f.file.clone(),
                start_line: f.start_line,
                end_line: f.end_line,
                complexity: f.complexity,
            })
            .collect();
        // Sort by descending complexity and trim.
        top_functions.sort_by(|a, b| b.complexity.cmp(&a.complexity));
        top_functions.truncate(10);

        let express_endpoints: Vec<EndpointSummary> = bake
            .endpoints
            .iter()
            .take(20)
            .map(|e| EndpointSummary {
                method: e.method.clone(),
                path: e.path.clone(),
                file: e.file.clone(),
                handler_name: e.handler_name.clone(),
            })
            .collect();

        let payload = ShakePayload {
            tool: "shake",
            version: env!("CARGO_PKG_VERSION"),
            project_root: root,
            languages: bake.languages.into_iter().collect(),
            files_indexed: bake.files.len(),
            notes: "Shake is using the bake index: languages, files, top complex functions, and Express endpoints are derived from .bakes/latest/bake.db.".to_string(),
            top_functions: Some(top_functions),
            express_endpoints: Some(express_endpoints),
        };

        let json = serde_json::to_string_pretty(&payload)?;
        Ok(json)
    } else {
        // Fallback: lightweight filesystem scan if no bake exists yet.
        let snapshot = project_snapshot(&root)?;

        let payload = ShakePayload {
            tool: "shake",
            version: env!("CARGO_PKG_VERSION"),
            project_root: root,
            languages: snapshot.languages.into_iter().collect(),
            files_indexed: snapshot.files_indexed,
            notes: "Shake is currently backed by a lightweight filesystem scan (languages + file counts). Run `bake` first to unlock richer summaries.".to_string(),
            top_functions: None,
            express_endpoints: None,
        };

        let json = serde_json::to_string_pretty(&payload)?;
        Ok(json)
    }
}

/// Public entrypoint for the `bake` tool: build and persist a basic project index.
///
/// This first version records files, languages, and sizes, and writes
/// `.bakes/latest/bake.db` under the project root.
pub fn bake(path: Option<String>) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bakes_dir = prepare_bake_artifacts_dir(&root)?;
    let bake_path = bakes_dir.join("bake.db");

    // Load fingerprints from existing DB for incremental mode.
    // Empty map → fresh full build.
    let fingerprints = if bake_path.exists() {
        crate::engine::db::load_file_fingerprints(&bake_path)
    } else {
        std::collections::HashMap::new()
    };
    let is_incremental = !fingerprints.is_empty();

    let (bake, removed, skipped) = build_bake_index(&root, &fingerprints)?;

    let (total_files, all_languages) = if is_incremental {
        crate::engine::db::write_bake_incremental(&bake, &removed, &bake_path).map_err(|e| {
            anyhow::anyhow!(
                "Failed to update bake index at {}: {}",
                bake_path.display(),
                e
            )
        })?
    } else {
        crate::engine::db::write_bake_to_db(&bake, &bake_path).map_err(|e| {
            anyhow::anyhow!(
                "Failed to write bake index to {}: {}",
                bake_path.display(),
                e
            )
        })?;
        (bake.files.len(), bake.languages.iter().cloned().collect())
    };

    // Build embeddings DB for semantic_search in a detached thread — best-effort,
    // never blocks the bake response. semantic_search reads from a separate file
    // so there is no read/write race with the bake.db we just wrote.
    // YOYO_SKIP_EMBED=1 disables the background build. Tests also skip it to avoid
    // detached worker races during short-lived tempdir-based runs.
    let bakes_dir_for_embed = bakes_dir.clone();
    if !cfg!(test) && std::env::var("YOYO_SKIP_EMBED").as_deref() != Ok("1") {
        std::thread::spawn(move || {
            if let Err(e) = crate::engine::embed::build_embeddings(&bakes_dir_for_embed) {
                eprintln!("[yoyo] Embeddings skipped: {e}");
            }
        });
    }

    let full_bake = load_bake_index(&root)?.unwrap_or(bake);
    let scopes = super::util::bake_scopes(&full_bake);
    let scope_dependencies = super::util::bake_scope_dependencies(&full_bake);
    let scoping_hints = super::util::scope_hints(&scopes, &scope_dependencies);

    let summary = BakeSummary {
        tool: "bake",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        bake_path,
        files_indexed: total_files,
        languages: all_languages,
        scopes,
        scope_dependencies,
        files_skipped: if skipped > 0 { Some(skipped) } else { None },
        scoping_hints,
    };

    let out = serde_json::to_string_pretty(&summary)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{llm_instructions, tool_help};
    use serde_json::Value;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn boot_includes_capabilities_and_common_tasks() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();

        let json = llm_instructions(Some(dir.path().to_string_lossy().into_owned())).unwrap();
        let payload: Value = serde_json::from_str(&json).unwrap();

        let capabilities = payload["capabilities"].as_array().unwrap();
        assert!(
            capabilities.iter().any(|entry| {
                entry["name"] == "relate"
                    && entry["tools"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|tool| tool == "impact")
            }),
            "boot should expose task-shaped capability groupings"
        );
        assert!(
            capabilities.iter().any(|entry| {
                entry["name"] == "judge"
                    && entry["tools"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|tool| tool == "judge_change")
            }),
            "boot should expose the high-level change-judgment capability"
        );

        let common_tasks = payload["common_tasks"].as_array().unwrap();
        assert!(
            common_tasks.iter().any(|entry| {
                entry["task"] == "Check whether a function is safe to delete"
                    && entry["use"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|tool| tool == "change")
            }),
            "boot should recommend concrete tools for common tasks"
        );
        assert!(
            common_tasks.iter().any(|entry| {
                entry["task"] == "Judge ownership, invariants, and regression risk before editing"
                    && entry["use"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|tool| tool == "judge_change")
            }),
            "boot should route change-triage questions to judge_change"
        );

        let user_config_files = payload["user_config_files"].as_array().unwrap();
        assert!(
            user_config_files.iter().any(|entry| {
                entry["path"] == "yoyo.json"
                    && entry["editable"] == true
                    && entry["git_trackable"] == true
                    && entry["agent_managed"] == true
                    && entry["description"]
                        .as_str()
                        .unwrap()
                        .contains("Agents can update")
            }),
            "boot should surface the agent-managed runtime config"
        );

        let runtime_access = &payload["runtime_access"];
        assert_eq!(runtime_access["config_path"], "yoyo.json");
        assert!(
            runtime_access["summary"]
                .as_str()
                .unwrap()
                .contains("update yoyo.json"),
            "boot should explain how to widen runtime access"
        );
        assert_eq!(runtime_access["git_trackable"], true);
        assert_eq!(runtime_access["agent_managed"], true);
        assert_eq!(runtime_access["recommended_action"]["tool"], "change");
        assert_eq!(runtime_access["recommended_action"]["action"], "edit");
        assert_eq!(runtime_access["recommended_action"]["file"], "yoyo.json");
        assert_eq!(
            runtime_access["enable_unsandboxed_example"]["runtime"]["checks"][0]
                ["allow_unsandboxed"],
            true
        );

        let project_conventions = &payload["project_conventions"];
        assert_eq!(project_conventions["configured"], false);
        assert_eq!(project_conventions["source"], "yoyo.json");

        let managed_paths = payload["managed_paths"].as_array().unwrap();
        assert!(
            managed_paths.iter().any(|entry| {
                entry["path"] == ".bakes/"
                    && entry["editable"] == false
                    && entry["description"]
                        .as_str()
                        .unwrap()
                        .contains("Do not edit")
            }),
            "boot should distinguish managed cache paths from user-edited config"
        );
    }

    #[test]
    fn boot_surfaces_project_conventions_from_yoyo_json() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
        fs::write(
            dir.path().join("yoyo.json"),
            r#"{
  "conventions": {
    "languages": ["rust"],
    "frameworks": ["axum"],
    "style_rules": ["prefer engine fixes over presentation workarounds"],
    "commands": {
      "test": ["cargo", "test"]
    }
  }
}"#,
        )
        .unwrap();

        let json = llm_instructions(Some(dir.path().to_string_lossy().into_owned())).unwrap();
        let payload: Value = serde_json::from_str(&json).unwrap();
        let conventions = &payload["project_conventions"];

        assert_eq!(conventions["configured"], true);
        assert_eq!(conventions["source"], "yoyo.json");
        assert_eq!(conventions["languages"][0], "rust");
        assert_eq!(conventions["frameworks"][0], "axum");
        assert_eq!(
            conventions["style_rules"][0],
            "prefer engine fixes over presentation workarounds"
        );
        assert_eq!(conventions["commands"]["test"][0], "cargo");
        assert_eq!(conventions["commands"]["test"][1], "test");
    }

    #[test]
    fn help_supports_task_topics() {
        let json = tool_help("safe delete".to_string()).unwrap();
        let payload: Value = serde_json::from_str(&json).unwrap();

        assert_eq!(payload["task"], "safe delete");
        assert!(payload["use"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool == "impact"));
        assert!(payload["use"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool == "change"));
    }

    #[test]
    fn help_supports_inspect_tool() {
        let json = tool_help("inspect".to_string()).unwrap();
        let payload: Value = serde_json::from_str(&json).unwrap();

        assert_eq!(payload["name"], "inspect");
        assert!(payload["limitations"]
            .as_str()
            .unwrap()
            .contains("line-range mode does not"));
    }

    #[test]
    fn help_supports_judge_change_tool() {
        let json = tool_help("judge_change".to_string()).unwrap();
        let payload: Value = serde_json::from_str(&json).unwrap();

        assert_eq!(payload["name"], "judge_change");
        assert!(payload["limitations"]
            .as_str()
            .unwrap()
            .contains("read-only judgment"));
        assert!(payload["output_shape"]
            .as_str()
            .unwrap()
            .contains("ownership_layer"));
    }

    #[test]
    fn help_supports_retry_plan_tool() {
        let json = tool_help("retry_plan".to_string()).unwrap();
        let payload: Value = serde_json::from_str(&json).unwrap();

        assert_eq!(payload["name"], "retry_plan");
        assert!(payload["limitations"]
            .as_str()
            .unwrap()
            .contains("does not repair code itself"));
        assert!(payload["output_shape"]
            .as_str()
            .unwrap()
            .contains("guard_failure"));
    }

    #[test]
    fn help_hides_legacy_mechanism_topics() {
        let err = tool_help("symbol".to_string()).unwrap_err();
        assert!(err.to_string().contains("Unknown help topic"));
        assert!(err.to_string().contains("inspect"));
    }
}
