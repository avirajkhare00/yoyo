use std::collections::BTreeMap;

use anyhow::{anyhow, Result};

use super::types::{
    AllEndpointsPayload, ApiSurfaceModule, ApiSurfacePayload, ApiTracePayload, CrudEntitySummary,
    CrudOperation, CrudOperationsPayload, EndpointSummary, FunctionSummary,
};
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
