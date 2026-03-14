use anyhow::{anyhow, Result};

use super::graph::trace_chain;
use super::types::{AllEndpointsPayload, EndpointSummary, FlowHandlerInfo, FlowPayload};
use super::util::{
    backend_scope_boost, bake_scope_dependencies, bake_scopes, effective_scope,
    is_backend_endpoint_task, matches_scope_filter, require_bake_index, resolve_project_root,
    scope_hints,
};

fn endpoint_match_score(
    endpoint: &crate::lang::IndexedEndpoint,
    query: Option<&str>,
    method: Option<&str>,
    prefer_backend: bool,
) -> Option<i32> {
    let method_uc = method.map(|value| value.to_uppercase());
    if method_uc
        .as_ref()
        .map(|value| endpoint.method != *value)
        .unwrap_or(false)
    {
        return None;
    }

    let mut score = 0i32;
    if let Some(query) = query {
        let needle = query.to_lowercase();
        let path_lc = endpoint.path.to_lowercase();
        let handler_lc = endpoint
            .handler_name
            .as_deref()
            .unwrap_or("")
            .to_lowercase();
        let file_lc = endpoint.file.to_lowercase();

        if path_lc == needle {
            score += 100;
        } else if path_lc.contains(&needle) {
            score += 70;
        } else if handler_lc.contains(&needle) || file_lc.contains(&needle) {
            score += 25;
        } else {
            return None;
        }
    }

    if prefer_backend {
        score += backend_scope_boost(&endpoint.file, &endpoint.language);
    }

    Some(score)
}

/// Public entrypoint for the `all_endpoints` tool: list Express-style endpoints.
pub fn all_endpoints(
    path: Option<String>,
    query: Option<String>,
    method: Option<String>,
    scope: Option<String>,
    limit: Option<usize>,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = require_bake_index(&root)?;
    let scopes = bake_scopes(&bake);
    let scope_dependencies = bake_scope_dependencies(&bake);
    let scoping_hints = scope_hints(&scopes, &scope_dependencies);
    let prefer_backend =
        is_backend_endpoint_task(query.as_deref(), query.as_deref(), None, scope.as_deref());
    let scope_used = effective_scope(
        &bake.files,
        scope.as_deref(),
        query.as_deref(),
        query.as_deref(),
        None,
        None,
    );

    let mut endpoints: Vec<(i32, EndpointSummary)> = bake
        .endpoints
        .iter()
        .filter(|endpoint| {
            matches_scope_filter(&endpoint.file, &endpoint.language, scope_used.as_deref())
        })
        .filter_map(|endpoint| {
            let score = endpoint_match_score(
                endpoint,
                query.as_deref(),
                method.as_deref(),
                prefer_backend,
            )?;
            Some((
                score,
                EndpointSummary {
                    method: endpoint.method.clone(),
                    path: endpoint.path.clone(),
                    file: endpoint.file.clone(),
                    handler_name: endpoint.handler_name.clone(),
                },
            ))
        })
        .collect();
    endpoints.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then(left.1.path.cmp(&right.1.path))
            .then(left.1.file.cmp(&right.1.file))
    });
    if let Some(limit) = limit {
        endpoints.truncate(limit);
    }
    let endpoints = endpoints
        .into_iter()
        .map(|(_, endpoint)| endpoint)
        .collect();

    let payload = AllEndpointsPayload {
        tool: "all_endpoints",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        endpoints,
        scope_used: scope_used.clone(),
        scoping_hints,
        next_hint: scope_used.map(|value| {
            format!("Use scope='{value}' again for backend endpoint work, or switch scopes if you need frontend/test routes.")
        }),
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

/// Public entrypoint for the `api_surface` tool: exported API summary by module (TypeScript-only for now).

/// Public entrypoint for the `api_trace` tool.

/// Public entrypoint for the `crud_operations` tool.

/// Public entrypoint for the `flow` tool: endpoint → handler → call chain in one call.
pub fn flow(
    path: Option<String>,
    endpoint: String,
    method: Option<String>,
    depth: Option<usize>,
    include_source: bool,
    scope: Option<String>,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = require_bake_index(&root)?;
    let scoping_hints = scope_hints(&bake_scopes(&bake), &bake_scope_dependencies(&bake));
    let scope_used = effective_scope(
        &bake.files,
        scope.as_deref(),
        Some(endpoint.as_str()),
        Some(endpoint.as_str()),
        None,
        None,
    );

    let ep = bake
        .endpoints
        .iter()
        .filter(|candidate| {
            matches_scope_filter(&candidate.file, &candidate.language, scope_used.as_deref())
        })
        .filter_map(|candidate| {
            endpoint_match_score(
                candidate,
                Some(endpoint.as_str()),
                method.as_deref(),
                true,
            )
            .map(|score| (score, candidate))
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, endpoint)| endpoint)
        .ok_or_else(|| {
            anyhow!(
                "No endpoint matching '{}'. Run `routes(query=\"{}\", scope=\"backend\")` or switch to symbol-first search/inspect if generated wiring hides the route.",
                endpoint,
                endpoint
            )
        })?;

    let handler_name = ep.handler_name.clone().ok_or_else(|| {
        anyhow!(
            "Endpoint '{}' has no resolved handler. It may use an inline/anonymous handler.",
            ep.path
        )
    })?;

    // Find handler function in index
    let handler_lc = handler_name.to_lowercase();
    let ep_file_lc = ep.file.to_lowercase();
    let start = {
        let mut matches: Vec<&crate::lang::IndexedFunction> = bake
            .functions
            .iter()
            .filter(|function| function.name.to_lowercase() == handler_lc)
            .collect();
        matches.sort_by(|left, right| {
            let left_file = left.file.to_lowercase();
            let right_file = right.file.to_lowercase();
            let left_same_file = left_file.contains(&ep_file_lc);
            let right_same_file = right_file.contains(&ep_file_lc);
            right_same_file.cmp(&left_same_file).then(
                backend_scope_boost(&right.file, &right.language)
                    .cmp(&backend_scope_boost(&left.file, &left.language)),
            )
        });
        matches
            .into_iter()
            .find(|function| {
                matches_scope_filter(&function.file, &function.language, scope_used.as_deref())
            })
            .or_else(|| {
                bake.functions
                    .iter()
                    .find(|function| function.name.to_lowercase() == handler_lc)
            })
    };

    let ep_summary = EndpointSummary {
        method: ep.method.clone(),
        path: ep.path.clone(),
        file: ep.file.clone(),
        handler_name: ep.handler_name.clone(),
    };

    let (handler_info, call_chain, boundaries, unresolved, chain_warning) = if let Some(start_fn) =
        start
    {
        let source = if include_source {
            std::fs::read_to_string(root.join(&start_fn.file))
                .ok()
                .and_then(|src| {
                    let lines: Vec<&str> = src.lines().collect();
                    let s = start_fn.start_line.saturating_sub(1) as usize;
                    let e = (start_fn.end_line as usize).saturating_sub(1).min(lines.len().saturating_sub(1));
                    const CAP: usize = 500;
                    if s >= lines.len() || s > e { return None; }
                    let total = e - s + 1;
                    if total > CAP {
                        let truncated = lines[s..s + CAP].join("\n");
                        Some(format!(
                            "{}\n... [truncated: {} lines total, showing first {}. Use slice(\"{}\", {}, {}) for the full body]",
                            truncated, total, CAP, start_fn.file, start_fn.start_line, start_fn.end_line,
                        ))
                    } else {
                        Some(lines[s..=e].join("\n"))
                    }
                })
        } else {
            None
        };

        let lang = start_fn.language.to_lowercase();
        let warning = if lang != "rust" && lang != "go" {
            Some(format!(
                "Call-chain tracing is not supported for {}. Handler returned but call_chain will be empty. Use supersearch (context=identifiers, pattern=call) to trace calls manually.",
                start_fn.language
            ))
        } else {
            None
        };

        let (chain, unresolved) = trace_chain(&bake, start_fn, depth.unwrap_or(5));
        let boundaries: Vec<String> = chain
            .iter()
            .filter_map(|n| n.boundary.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        let handler = FlowHandlerInfo {
            name: start_fn.name.clone(),
            file: start_fn.file.clone(),
            start_line: start_fn.start_line,
            source,
        };
        (handler, chain, boundaries, unresolved, warning)
    } else {
        let handler = FlowHandlerInfo {
            name: handler_name.clone(),
            file: ep.file.clone(),
            start_line: 0,
            source: None,
        };
        (handler, vec![], vec![], vec![], None)
    };

    let boundary_str = if boundaries.is_empty() {
        String::new()
    } else {
        format!(" → [{}]", boundaries.join(", "))
    };
    let chain_str = call_chain
        .iter()
        .filter(|n| n.depth > 0 && n.resolved)
        .map(|n| n.name.as_str())
        .collect::<Vec<_>>()
        .join(" → ");
    let summary = if chain_str.is_empty() {
        format!(
            "{} {} → {}{}",
            ep_summary.method, ep_summary.path, handler_info.name, boundary_str
        )
    } else {
        format!(
            "{} {} → {} → {}{}",
            ep_summary.method, ep_summary.path, handler_info.name, chain_str, boundary_str
        )
    };

    let payload = FlowPayload {
        tool: "flow",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        endpoint: ep_summary,
        handler: handler_info,
        scope_used,
        call_chain,
        boundaries,
        unresolved,
        summary,
        chain_warning,
        scoping_hints,
        next_hint: Some("Use inspect(name=...) on the handler or a downstream callee to read the code behind this route."),
    };
    Ok(serde_json::to_string_pretty(&payload)?)
}

/// Public entrypoint for the `impact` tool: understand symbol or endpoint impact
/// from one task-shaped surface.
pub fn impact(
    path: Option<String>,
    symbol: Option<String>,
    endpoint: Option<String>,
    method: Option<String>,
    depth: Option<usize>,
    include_source: Option<bool>,
    scope: Option<String>,
) -> Result<String> {
    let root = resolve_project_root(path.clone())?;

    let (mode, target, delegated, next_hint) = if let Some(symbol) = symbol {
        if endpoint.is_some() {
            return Err(anyhow!(
                "impact accepts either symbol or endpoint, not both"
            ));
        }
        (
            "symbol",
            serde_json::json!({
                "symbol": symbol,
            }),
            crate::engine::blast_radius(path, symbol, depth)?,
            "Use inspect(name=...) to read one affected caller, or change(action=rename|move|delete) when the impact is acceptable.",
        )
    } else if let Some(endpoint) = endpoint {
        (
            "endpoint",
            serde_json::json!({
                "endpoint": endpoint,
                "method": method,
                "scope": scope,
            }),
            crate::engine::flow(
                path,
                endpoint,
                method,
                depth,
                include_source.unwrap_or(false),
                scope,
            )?,
            "Use inspect(name=...) on the handler or downstream callee, or change(action=edit|rename|move) once you know where the request path lands.",
        )
    } else {
        return Err(anyhow!(
            "impact requires either symbol=<name> or endpoint=<path substring>"
        ));
    };

    let mut parsed = serde_json::from_str::<serde_json::Value>(&delegated)?
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("impact expected object payload from delegated relation tool"))?;
    parsed.remove("tool");
    parsed.remove("version");
    parsed.remove("project_root");
    parsed.remove("next_hint");

    let mut payload = serde_json::Map::new();
    payload.insert("tool".to_string(), serde_json::json!("impact"));
    payload.insert(
        "version".to_string(),
        serde_json::json!(env!("CARGO_PKG_VERSION")),
    );
    payload.insert("project_root".to_string(), serde_json::json!(root));
    payload.insert("mode".to_string(), serde_json::json!(mode));
    payload.insert("target".to_string(), target);
    payload.extend(parsed);
    payload.insert("next_hint".to_string(), serde_json::json!(next_hint));

    Ok(serde_json::to_string_pretty(&serde_json::Value::Object(
        payload,
    ))?)
}
