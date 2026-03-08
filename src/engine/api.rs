use std::collections::BTreeMap;

use anyhow::{anyhow, Result};

use super::types::{
    AllEndpointsPayload, ApiSurfaceModule, ApiSurfacePayload, ApiTracePayload, CrudEntitySummary,
    CrudOperation, CrudOperationsPayload, EndpointSummary, FlowHandlerInfo, FlowPayload,
    FunctionSummary,
};
use super::graph::trace_chain;
use super::util::{infer_entity_from_path, load_bake_index, module_from_path, resolve_project_root};

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
pub fn api_surface(
    path: Option<String>,
    package: Option<String>,
    limit: Option<usize>,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index found. Run `bake` first to build bakes/latest/bake.json."))?;

    let limit = limit.unwrap_or(20);
    let package_filter = package.clone().map(|p| p.to_lowercase());

    let mut modules: BTreeMap<String, Vec<FunctionSummary>> = BTreeMap::new();

    for f in &bake.functions {
        let module = module_from_path(&f.file);
        if let Some(ref pf) = package_filter {
            if !module.to_lowercase().contains(pf) && !f.file.to_lowercase().contains(pf) {
                continue;
            }
        }

        modules
            .entry(module)
            .or_default()
            .push(FunctionSummary {
                name: f.name.clone(),
                file: f.file.clone(),
                start_line: f.start_line,
                end_line: f.end_line,
                complexity: f.complexity,
            });
    }

    let total_modules = modules.len();

    let mut modules_vec: Vec<ApiSurfaceModule> = modules
        .into_iter()
        .map(|(module, mut functions)| {
            functions.sort_by(|a, b| b.complexity.cmp(&a.complexity));
            functions.truncate(limit);
            ApiSurfaceModule { module, functions }
        })
        .collect();

    modules_vec.sort_by(|a, b| a.module.cmp(&b.module));
    modules_vec.truncate(limit);
    let truncated = total_modules > limit;

    let payload = ApiSurfacePayload {
        tool: "api_surface",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        package,
        limit,
        total_modules,
        truncated,
        modules: modules_vec,
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

/// Public entrypoint for the `api_trace` tool.
pub fn api_trace(
    path: Option<String>,
    endpoint: String,
    method: Option<String>,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index found. Run `bake` first to build bakes/latest/bake.json."))?;

    let method_lc = method.clone().map(|m| m.to_uppercase());
    let endpoint_lc = endpoint.to_lowercase();

    let mut traces = Vec::new();

    for e in &bake.endpoints {
        if !e.path.to_lowercase().contains(&endpoint_lc) {
            continue;
        }
        if let Some(ref m) = method_lc {
            if &e.method != m {
                continue;
            }
        }

        traces.push(EndpointSummary {
            method: e.method.clone(),
            path: e.path.clone(),
            file: e.file.clone(),
            handler_name: e.handler_name.clone(),
        });
    }

    let payload = ApiTracePayload {
        tool: "api_trace",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        endpoint,
        method: method_lc,
        traces,
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

/// Public entrypoint for the `crud_operations` tool.
pub fn crud_operations(path: Option<String>, entity: Option<String>) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index found. Run `bake` first to build bakes/latest/bake.json."))?;

    let entity_filter = entity.clone().map(|e| e.to_lowercase());
    let mut entities: BTreeMap<String, CrudEntitySummary> = BTreeMap::new();

    for e in &bake.endpoints {
        let path_seg = infer_entity_from_path(&e.path);
        if path_seg.is_empty() {
            continue;
        }
        if let Some(ref ef) = entity_filter {
            if !path_seg.to_lowercase().contains(ef) {
                continue;
            }
        }

        let entry = entities.entry(path_seg.clone()).or_insert_with(|| CrudEntitySummary {
            entity: path_seg.clone(),
            operations: Vec::new(),
        });

        let op = match e.method.as_str() {
            "GET" => "read",
            "POST" => "create",
            "PUT" | "PATCH" => "update",
            "DELETE" => "delete",
            _ => "other",
        };

        entry.operations.push(CrudOperation {
            operation: op.to_string(),
            method: e.method.clone(),
            path: e.path.clone(),
            file: e.file.clone(),
        });
    }

    let mut entities_vec: Vec<CrudEntitySummary> = entities.into_values().collect();
    entities_vec.sort_by(|a, b| a.entity.cmp(&b.entity));

    let payload = CrudOperationsPayload {
        tool: "crud_operations",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        entity,
        entities: entities_vec,
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

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
