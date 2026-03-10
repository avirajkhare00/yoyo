use anyhow::{anyhow, Result};

use super::types::{AllEndpointsPayload, EndpointSummary, FlowHandlerInfo, FlowPayload};
use super::graph::trace_chain;
use super::util::{load_bake_index, resolve_project_root};

/// Public entrypoint for the `all_endpoints` tool: list Express-style endpoints.
pub fn all_endpoints(path: Option<String>) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index found. Run `bake` first to build bakes/latest/bake.json."))?;

    let endpoints: Vec<EndpointSummary> = bake
        .endpoints
        .iter()
        .map(|e| EndpointSummary {
            method: e.method.clone(),
            path: e.path.clone(),
            file: e.file.clone(),
            handler_name: e.handler_name.clone(),
        })
        .collect();

    let payload = AllEndpointsPayload {
        tool: "all_endpoints",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        endpoints,
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
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index found. Run `bake` first."))?;

    let method_uc = method.map(|m| m.to_uppercase());
    let endpoint_lc = endpoint.to_lowercase();

    // Find matching endpoint
    let ep = bake.endpoints.iter().find(|e| {
        e.path.to_lowercase().contains(&endpoint_lc)
            && method_uc.as_ref().map(|m| &e.method == m).unwrap_or(true)
    }).ok_or_else(|| anyhow!("No endpoint matching '{}'. Run `all_endpoints` to list available routes.", endpoint))?;

    let handler_name = ep.handler_name.clone()
        .ok_or_else(|| anyhow!("Endpoint '{}' has no resolved handler. It may use an inline/anonymous handler.", ep.path))?;

    // Find handler function in index
    let handler_lc = handler_name.to_lowercase();
    let ep_file_lc = ep.file.to_lowercase();
    let start = bake.functions.iter().find(|f| {
        f.name.to_lowercase() == handler_lc
            && f.file.to_lowercase().contains(&ep_file_lc)
    }).or_else(|| bake.functions.iter().find(|f| f.name.to_lowercase() == handler_lc));

    let ep_summary = EndpointSummary {
        method: ep.method.clone(),
        path: ep.path.clone(),
        file: ep.file.clone(),
        handler_name: ep.handler_name.clone(),
    };

    let (handler_info, call_chain, boundaries, unresolved, chain_warning) = if let Some(start_fn) = start {
        let source = if include_source {
            std::fs::read_to_string(root.join(&start_fn.file))
                .ok()
                .and_then(|src| {
                    let lines: Vec<&str> = src.lines().collect();
                    let s = start_fn.start_line.saturating_sub(1) as usize;
                    let e = (start_fn.end_line as usize).min(lines.len());
                    if s < lines.len() { Some(lines[s..e].join("\n")) } else { None }
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
        let boundaries: Vec<String> = chain.iter()
            .filter_map(|n| n.boundary.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter().collect();

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
    let chain_str = call_chain.iter()
        .filter(|n| n.depth > 0 && n.resolved)
        .map(|n| n.name.as_str())
        .collect::<Vec<_>>()
        .join(" → ");
    let summary = if chain_str.is_empty() {
        format!("{} {} → {}{}", ep_summary.method, ep_summary.path, handler_info.name, boundary_str)
    } else {
        format!("{} {} → {} → {}{}", ep_summary.method, ep_summary.path, handler_info.name, chain_str, boundary_str)
    };

    let payload = FlowPayload {
        tool: "flow",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        endpoint: ep_summary,
        handler: handler_info,
        call_chain,
        boundaries,
        unresolved,
        summary,
        chain_warning,
    };
    Ok(serde_json::to_string_pretty(&payload)?)
}
