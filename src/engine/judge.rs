use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use anyhow::Result;

use super::types::{
    JudgeCandidateFile, JudgeCandidateSymbol, JudgeChangePayload, JudgeCommand, JudgeFinding,
    JudgeOwnershipLayer, JudgeRejectedAlternative,
};
use super::util::{
    backend_scope_boost, bake_artifacts_dir, bake_scope_dependencies, bake_scopes, effective_scope,
    matches_scope_filter, require_bake_index, resolve_project_root, scope_hints,
};

#[derive(Clone)]
struct Candidate {
    name: String,
    file: String,
    start_line: u32,
    kind: &'static str,
    score: f32,
    why_parts: BTreeSet<String>,
    incoming_callers: usize,
    caller_files: usize,
    parent_type: Option<String>,
    visibility: Option<String>,
}

pub fn judge_change(
    path: Option<String>,
    query: String,
    symbol: Option<String>,
    file: Option<String>,
    limit: Option<usize>,
    scope: Option<String>,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = require_bake_index(&root)?;
    let limit = limit.unwrap_or(3).clamp(1, 5);
    let file_filter = file.as_deref().map(str::to_lowercase);
    let scope_used = effective_scope(
        &bake.files,
        scope.as_deref(),
        Some(query.as_str()),
        None,
        symbol.as_deref(),
        file.as_deref(),
    );
    let scoping_hints = scope_hints(&bake_scopes(&bake), &bake_scope_dependencies(&bake));

    let mut caller_names_by_symbol: HashMap<String, HashSet<String>> = HashMap::new();
    let mut caller_files_by_symbol: HashMap<String, HashSet<String>> = HashMap::new();
    for func in &bake.functions {
        for call in &func.calls {
            caller_names_by_symbol
                .entry(call.callee.to_lowercase())
                .or_default()
                .insert(func.name.clone());
            caller_files_by_symbol
                .entry(call.callee.to_lowercase())
                .or_default()
                .insert(func.file.clone());
        }
    }

    let mut candidates: BTreeMap<(String, String, u32, &'static str), Candidate> = BTreeMap::new();

    if let Some(ref hint) = symbol {
        let needle = hint.to_lowercase();
        for func in bake
            .functions
            .iter()
            .filter(|f| f.name.to_lowercase() == needle)
        {
            if matches_file_filter(&func.file, file_filter.as_deref())
                && matches_scope_filter(&func.file, &func.language, scope_used.as_deref())
            {
                upsert_candidate(
                    &mut candidates,
                    Candidate {
                        name: func.name.clone(),
                        file: func.file.clone(),
                        start_line: func.start_line,
                        kind: "function",
                        score: 100.0 + backend_scope_boost(&func.file, &func.language) as f32,
                        why_parts: BTreeSet::from([format!(
                            "Matches explicit symbol hint '{}'.",
                            hint
                        )]),
                        incoming_callers: caller_names_by_symbol
                            .get(&needle)
                            .map(|s| s.len())
                            .unwrap_or(0),
                        caller_files: caller_files_by_symbol
                            .get(&needle)
                            .map(|s| s.len())
                            .unwrap_or(0),
                        parent_type: func.parent_type.clone(),
                        visibility: Some(visibility_label(&func.visibility).to_string()),
                    },
                );
            }
        }

        for ty in bake
            .types
            .iter()
            .filter(|t| t.name.to_lowercase() == needle)
        {
            if matches_file_filter(&ty.file, file_filter.as_deref())
                && matches_scope_filter(&ty.file, &ty.language, scope_used.as_deref())
            {
                upsert_candidate(
                    &mut candidates,
                    Candidate {
                        name: ty.name.clone(),
                        file: ty.file.clone(),
                        start_line: ty.start_line,
                        kind: "type",
                        score: 95.0 + backend_scope_boost(&ty.file, &ty.language) as f32,
                        why_parts: BTreeSet::from([format!(
                            "Matches explicit symbol hint '{}'.",
                            hint
                        )]),
                        incoming_callers: 0,
                        caller_files: 0,
                        parent_type: None,
                        visibility: Some(visibility_label(&ty.visibility).to_string()),
                    },
                );
            }
        }
    }

    let semantic_limit = limit.max(3) * 4;
    for (score, func) in semantic_candidates(
        &root,
        &bake,
        &query,
        file_filter.as_deref(),
        scope_used.as_deref(),
        semantic_limit,
    ) {
        upsert_candidate(
            &mut candidates,
            Candidate {
                name: func.name.clone(),
                file: func.file.clone(),
                start_line: func.start_line,
                kind: "function",
                score,
                why_parts: BTreeSet::from([format!(
                    "Intent query matched this function's name/module/callees (score {:.2}).",
                    score
                )]),
                incoming_callers: caller_names_by_symbol
                    .get(&func.name.to_lowercase())
                    .map(|s| s.len())
                    .unwrap_or(0),
                caller_files: caller_files_by_symbol
                    .get(&func.name.to_lowercase())
                    .map(|s| s.len())
                    .unwrap_or(0),
                parent_type: func.parent_type.clone(),
                visibility: Some(visibility_label(&func.visibility).to_string()),
            },
        );
    }

    let mut ranked: Vec<Candidate> = candidates.into_values().collect();
    ranked.sort_by(rank_candidates);

    if ranked.is_empty() {
        let verification_commands = verification_commands(&root, None);
        let payload = JudgeChangePayload {
            tool: "judge_change",
            version: env!("CARGO_PKG_VERSION"),
            project_root: root,
            query,
            symbol_hint: symbol,
            file_hint: file,
            ownership_layer: JudgeOwnershipLayer {
                name: "unknown".to_string(),
                why: "No strongly grounded candidates were found for this query. Narrow with a symbol or file hint, or inspect the likely area first.".to_string(),
                evidence_files: vec![],
            },
            scope_used,
            candidate_symbols: vec![],
            candidate_files: vec![],
            rejected_alternatives: vec![],
            invariants: vec![],
            regression_risks: vec![],
            verification_commands,
            scoping_hints,
            next_hint: Some("Add a symbol or file hint, or use inspect(...) on the likely area before changing code."),
        };
        return Ok(serde_json::to_string_pretty(&payload)?);
    }

    let top_candidates: Vec<Candidate> = ranked.iter().take(limit).cloned().collect();
    let rejected_alternatives: Vec<JudgeRejectedAlternative> = ranked
        .iter()
        .skip(limit)
        .take(2)
        .map(|c| JudgeRejectedAlternative {
            name: c.name.clone(),
            file: c.file.clone(),
            start_line: c.start_line,
            kind: c.kind,
            reason: if c.score < top_candidates[0].score {
                format!(
                    "Lower ranked than the top ownership candidate ({:.2} vs {:.2}).",
                    c.score, top_candidates[0].score
                )
            } else {
                "Relevant, but less central to the judged ownership layer.".to_string()
            },
        })
        .collect();

    let candidate_symbols: Vec<JudgeCandidateSymbol> = top_candidates
        .iter()
        .map(|c| JudgeCandidateSymbol {
            name: c.name.clone(),
            file: c.file.clone(),
            start_line: c.start_line,
            kind: c.kind,
            score: round2(c.score),
            why: c.why_parts.iter().cloned().collect::<Vec<_>>().join(" "),
            incoming_callers: c.incoming_callers,
            caller_files: c.caller_files,
            parent_type: c.parent_type.clone(),
            visibility: c.visibility.clone(),
        })
        .collect();

    let candidate_files = candidate_files(&top_candidates);
    let ownership_layer = ownership_layer(&top_candidates);
    let invariants = invariants(&ownership_layer, &top_candidates, &candidate_files);
    let regression_risks = regression_risks(&ownership_layer, &top_candidates, &candidate_files);
    let verification_commands = verification_commands(&root, top_candidates.first());

    let payload = JudgeChangePayload {
        tool: "judge_change",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        query,
        symbol_hint: symbol,
        file_hint: file,
        scope_used,
        ownership_layer,
        candidate_symbols,
        candidate_files,
        rejected_alternatives,
        invariants,
        regression_risks,
        verification_commands,
        scoping_hints,
        next_hint: Some("Use change(action=...) once you accept the ownership layer, or inspect(...) to drill into one candidate before writing."),
    };

    Ok(serde_json::to_string_pretty(&payload)?)
}

fn semantic_candidates<'a>(
    root: &std::path::Path,
    bake: &'a crate::engine::types::BakeIndex,
    query: &str,
    file_filter: Option<&str>,
    scope_filter: Option<&str>,
    limit: usize,
) -> Vec<(f32, &'a crate::lang::IndexedFunction)> {
    let bake_dir = bake_artifacts_dir(&root);
    if let Ok(Some(matches)) =
        crate::engine::embed::vector_search(&bake_dir, query, limit, file_filter)
    {
        let lookup: HashMap<(&str, &str, u32), &crate::lang::IndexedFunction> = bake
            .functions
            .iter()
            .map(|f| ((f.name.as_str(), f.file.as_str(), f.start_line), f))
            .collect();
        let mut ranked = Vec::new();
        for m in matches {
            if let Some(func) = lookup.get(&(m.name.as_str(), m.file.as_str(), m.start_line)) {
                if matches_scope_filter(&func.file, &func.language, scope_filter) {
                    ranked.push((
                        m.score * 10.0 + backend_scope_boost(&func.file, &func.language) as f32,
                        *func,
                    ));
                }
            }
        }
        if !ranked.is_empty() {
            return ranked;
        }
    }

    let query_tokens = tokenize(query);
    if query_tokens.is_empty() {
        return vec![];
    }

    let n = bake.functions.len() as f32;
    let mut doc_freq: HashMap<String, f32> = HashMap::new();
    for func in &bake.functions {
        for tok in tokenize(&func.name).into_iter().collect::<HashSet<_>>() {
            *doc_freq.entry(tok).or_insert(0.0) += 1.0;
        }
    }
    let idf = |tok: &str| -> f32 {
        let df = doc_freq.get(tok).copied().unwrap_or(0.0);
        ((n + 1.0) / (df + 1.0)).ln() + 1.0
    };

    let mut ranked: Vec<(f32, &crate::lang::IndexedFunction)> = bake
        .functions
        .iter()
        .filter(|f| matches_file_filter(&f.file, file_filter))
        .filter(|f| matches_scope_filter(&f.file, &f.language, scope_filter))
        .filter_map(|f| {
            let score =
                score_fn(f, &query_tokens, &idf) + backend_scope_boost(&f.file, &f.language) as f32;
            (score > 0.0).then_some((score, f))
        })
        .collect();
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
    ranked.truncate(limit);
    ranked
}

fn upsert_candidate(
    candidates: &mut BTreeMap<(String, String, u32, &'static str), Candidate>,
    candidate: Candidate,
) {
    let key = (
        candidate.name.clone(),
        candidate.file.clone(),
        candidate.start_line,
        candidate.kind,
    );
    candidates
        .entry(key)
        .and_modify(|existing| {
            if candidate.score > existing.score {
                existing.score = candidate.score;
            }
            existing.incoming_callers = existing.incoming_callers.max(candidate.incoming_callers);
            existing.caller_files = existing.caller_files.max(candidate.caller_files);
            if existing.parent_type.is_none() {
                existing.parent_type = candidate.parent_type.clone();
            }
            if existing.visibility.is_none() {
                existing.visibility = candidate.visibility.clone();
            }
            existing
                .why_parts
                .extend(candidate.why_parts.iter().cloned());
        })
        .or_insert(candidate);
}

fn rank_candidates(a: &Candidate, b: &Candidate) -> Ordering {
    b.score
        .partial_cmp(&a.score)
        .unwrap_or(Ordering::Equal)
        .then_with(|| b.caller_files.cmp(&a.caller_files))
        .then_with(|| b.incoming_callers.cmp(&a.incoming_callers))
        .then_with(|| a.file.cmp(&b.file))
        .then_with(|| a.start_line.cmp(&b.start_line))
}

fn candidate_files(candidates: &[Candidate]) -> Vec<JudgeCandidateFile> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for candidate in candidates {
        *counts.entry(candidate.file.as_str()).or_insert(0) += 1;
    }

    counts
        .into_iter()
        .map(|(file, symbol_count)| JudgeCandidateFile {
            file: file.to_string(),
            role: if symbol_count > 1 {
                "cluster".to_string()
            } else {
                "primary".to_string()
            },
            why: if symbol_count > 1 {
                format!(
                    "Multiple top-ranked symbols cluster in this file ({} candidates).",
                    symbol_count
                )
            } else {
                "Contains a top-ranked ownership candidate.".to_string()
            },
        })
        .collect()
}

fn ownership_layer(candidates: &[Candidate]) -> JudgeOwnershipLayer {
    let mut directories: Vec<String> = candidates
        .iter()
        .map(|c| {
            Path::new(&c.file)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| c.file.clone())
        })
        .collect();
    directories.sort();
    directories.dedup();

    let common = common_path_prefix(&directories);
    let name = if !common.is_empty() {
        common
    } else {
        directories
            .first()
            .cloned()
            .unwrap_or_else(|| "project root".to_string())
    };
    let evidence_files: Vec<String> = candidates.iter().map(|c| c.file.clone()).collect();
    let why = if evidence_files.len() > 1 {
        format!(
            "Top-ranked candidates cluster under {} across {} files, so that is the narrowest grounded ownership layer.",
            name,
            evidence_files.len()
        )
    } else {
        format!(
            "The strongest grounded candidate lives in {}, so that is the most likely ownership layer.",
            name
        )
    };

    JudgeOwnershipLayer {
        name,
        why,
        evidence_files,
    }
}

fn invariants(
    ownership: &JudgeOwnershipLayer,
    candidates: &[Candidate],
    candidate_files: &[JudgeCandidateFile],
) -> Vec<JudgeFinding> {
    let mut findings = Vec::new();
    let top = &candidates[0];

    findings.push(JudgeFinding {
        text: format!(
            "Keep the fix owned in {} instead of pushing the behavior into callers or unrelated layers.",
            ownership.name
        ),
        evidence: ownership.evidence_files.clone(),
    });

    if top.caller_files > 0 {
        findings.push(JudgeFinding {
            text: format!(
                "Preserve the caller contract of {} across {} calling files.",
                top.name, top.caller_files
            ),
            evidence: vec![format!("{}:{}", top.file, top.start_line)],
        });
    }

    if let Some(ref visibility) = top.visibility {
        if visibility != "private" {
            findings.push(JudgeFinding {
                text: format!(
                    "Preserve the {} surface and signature expectations around {}.",
                    visibility, top.name
                ),
                evidence: vec![format!("{}:{}", top.file, top.start_line)],
            });
        }
    }

    if candidate_files.len() > 1 {
        findings.push(JudgeFinding {
            text: "Keep cross-file behavior consistent inside the judged ownership layer; do not fix one file in a way that invalidates its sibling contract.".to_string(),
            evidence: candidate_files.iter().map(|f| f.file.clone()).collect(),
        });
    }

    findings
}

fn regression_risks(
    ownership: &JudgeOwnershipLayer,
    candidates: &[Candidate],
    candidate_files: &[JudgeCandidateFile],
) -> Vec<JudgeFinding> {
    let mut findings = Vec::new();
    let top = &candidates[0];

    if top.caller_files > 0 {
        findings.push(JudgeFinding {
            text: format!(
                "Changing {} can fan out to {} callers across {} files.",
                top.name, top.incoming_callers, top.caller_files
            ),
            evidence: vec![format!("{}:{}", top.file, top.start_line)],
        });
    }

    if candidate_files.len() > 1 {
        findings.push(JudgeFinding {
            text: format!(
                "The likely fix surface spans {} files under {}, so a partial fix can leave the layer internally inconsistent.",
                candidate_files.len(),
                ownership.name
            ),
            evidence: candidate_files.iter().map(|f| f.file.clone()).collect(),
        });
    }

    if let Some(ref visibility) = top.visibility {
        if visibility == "public" || visibility == "module" {
            findings.push(JudgeFinding {
                text: format!(
                    "{} is {}-visible, so behavior changes can leak into downstream modules and tests.",
                    top.name, visibility
                ),
                evidence: vec![format!("{}:{}", top.file, top.start_line)],
            });
        }
    }

    if findings.is_empty() {
        findings.push(JudgeFinding {
            text: format!(
                "The main risk is changing the wrong layer; keep the edit scoped to {} until the ownership seam is confirmed.",
                ownership.name
            ),
            evidence: ownership.evidence_files.clone(),
        });
    }

    findings
}

fn verification_commands(root: &Path, top: Option<&Candidate>) -> Vec<JudgeCommand> {
    let mut commands = vec![JudgeCommand {
        command: "git diff --stat".to_string(),
        why: "Confirm the final scope stays inside the judged ownership layer before broadening the patch.".to_string(),
    }];

    let top_name = top.map(|c| c.name.as_str()).unwrap_or("the judged area");
    if root.join("Cargo.toml").exists() {
        commands.push(JudgeCommand {
            command: "cargo test".to_string(),
            why: format!(
                "Project uses Cargo; rerun the Rust test suite after changing {}.",
                top_name
            ),
        });
    } else if root.join("go.mod").exists() {
        commands.push(JudgeCommand {
            command: "go test ./...".to_string(),
            why: format!(
                "Project uses Go modules; rerun package tests after changing {}.",
                top_name
            ),
        });
    } else if root.join("pyproject.toml").exists() || root.join("pytest.ini").exists() {
        commands.push(JudgeCommand {
            command: "pytest".to_string(),
            why: format!(
                "Project uses pytest-style Python tests; rerun them after changing {}.",
                top_name
            ),
        });
    } else if root.join("package.json").exists() {
        commands.push(JudgeCommand {
            command: "npm test".to_string(),
            why: format!(
                "Project has package.json; rerun JS/TS tests after changing {}.",
                top_name
            ),
        });
    }

    commands
}

fn common_path_prefix(paths: &[String]) -> String {
    let mut iter = paths.iter();
    let Some(first) = iter.next() else {
        return String::new();
    };
    let mut prefix: Vec<&str> = first.split('/').filter(|s| !s.is_empty()).collect();
    for path in iter {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let shared = prefix
            .iter()
            .zip(parts.iter())
            .take_while(|(a, b)| a == b)
            .count();
        prefix.truncate(shared);
        if prefix.is_empty() {
            break;
        }
    }
    prefix.join("/")
}

fn matches_file_filter(file: &str, file_filter: Option<&str>) -> bool {
    file_filter.map_or(true, |ff| file.to_lowercase().contains(ff))
}

fn visibility_label(vis: &crate::lang::Visibility) -> &'static str {
    match vis {
        crate::lang::Visibility::Public => "public",
        crate::lang::Visibility::Module => "module",
        crate::lang::Visibility::Private => "private",
    }
}

fn round2(score: f32) -> f32 {
    (score * 100.0).round() / 100.0
}

fn tokenize(s: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for c in s.chars() {
        if matches!(c, '_' | '-' | ' ' | '/' | '.' | ':') {
            if !current.is_empty() {
                tokens.push(current.to_lowercase());
                current.clear();
            }
        } else if c.is_uppercase() && !current.is_empty() {
            tokens.push(current.to_lowercase());
            current.clear();
            current.push(c);
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        tokens.push(current.to_lowercase());
    }
    tokens.into_iter().filter(|t| t.len() >= 2).collect()
}

fn score_fn<F: Fn(&str) -> f32>(
    func: &crate::lang::IndexedFunction,
    query_tokens: &[String],
    idf: F,
) -> f32 {
    let name_set: HashSet<String> = tokenize(&func.name).into_iter().collect();
    let callee_set: HashSet<String> = func
        .calls
        .iter()
        .flat_map(|c| tokenize(&c.callee))
        .collect();
    let module_set: HashSet<String> = tokenize(&func.module_path).into_iter().collect();
    let file_set: HashSet<String> = tokenize(&func.file).into_iter().collect();

    let mut score = 0.0f32;
    for qt in query_tokens {
        let weight = idf(qt);
        if name_set.contains(qt) {
            score += 3.0 * weight;
        }
        if module_set.contains(qt) {
            score += 1.5 * weight;
        }
        if callee_set.contains(qt) {
            score += 1.0 * weight;
        }
        if file_set.contains(qt) {
            score += 0.5 * weight;
        }
    }
    score
}
