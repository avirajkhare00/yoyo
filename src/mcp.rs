use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::OnceLock;
use tokio::io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

/// Run a minimal MCP-compatible JSON-RPC server over stdin/stdout.
///
/// Supports both:
/// - Line-delimited JSON-RPC (Claude Desktop currently does this).
/// - `Content-Length` framed JSON-RPC 2.0 messages (per MCP spec).
pub async fn run_stdio_server() -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut writer = stdout;

    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Ok(());
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            let mut content_length: Option<usize> = None;
            if let Ok(len) = rest.trim().parse::<usize>() {
                content_length = Some(len);
            }

            loop {
                let mut hdr = String::new();
                let n = reader.read_line(&mut hdr).await?;
                if n == 0 {
                    return Ok(());
                }
                if hdr.trim().is_empty() {
                    break;
                }
            }

            let len = match content_length {
                Some(l) => l,
                None => continue,
            };

            let mut buf = vec![0u8; len];
            reader.read_exact(&mut buf).await?;
            let body = match String::from_utf8(buf) {
                Ok(s) => s,
                Err(err) => {
                    eprintln!("[yoyo-mcp] Non-UTF8 JSON-RPC body: {err}");
                    continue;
                }
            };

            let req: JsonRpcRequest = match serde_json::from_str(&body) {
                Ok(r) => r,
                Err(err) => {
                    eprintln!("[yoyo-mcp] Failed to parse framed request: {err}");
                    continue;
                }
            };

            // Notifications (no id) are silently dropped — correct per spec.
            // Explicitly skip rather than falling through to handle_request.
            if req.id.is_none() {
                continue;
            }

            let resp = handle_request(req).await;
            let json = serde_json::to_string(&resp)?;
            let bytes = json.as_bytes();
            let header = format!("Content-Length: {}\r\n\r\n", bytes.len());
            writer.write_all(header.as_bytes()).await?;
            writer.write_all(bytes).await?;
            writer.flush().await?;
        } else if trimmed.starts_with('{') || trimmed.starts_with('[') {
            let body = trimmed.to_string();

            let req: JsonRpcRequest = match serde_json::from_str(&body) {
                Ok(r) => r,
                Err(err) => {
                    eprintln!("[yoyo-mcp] Failed to parse line-delimited request: {err}");
                    continue;
                }
            };

            // Notifications (no id) are silently dropped — correct per spec.
            // Explicitly skip rather than falling through to handle_request.
            if req.id.is_none() {
                continue;
            }

            let resp = handle_request(req).await;
            let json = serde_json::to_string(&resp)?;
            writer.write_all(json.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        } else {
            continue;
        }
    }
}

async fn handle_request(req: JsonRpcRequest) -> JsonRpcResponse {
    match req.method.as_str() {
        "initialize" => {
            let protocol_version = req
                .params
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("2025-11-25");

            let n_tools = crate::engine::tool_catalog().len();
            let instructions = format!(
                "yoyo: {n_tools} AST-grounded code intelligence tools. \
                    Think in tasks, not raw primitives: orient with boot/index/map/routes/health; locate with inspect/search/ask; judge with judge_change; relate with impact/routes/health; change with change. \
                    ALWAYS use yoyo tools INSTEAD OF built-ins when they fit: judge_change INSTEAD OF chaining search/inspect/impact manually for ownership, invariants, regression risk, and verification planning. inspect INSTEAD OF jumping between file reads and symbol lookup. search INSTEAD OF Grep/rg. impact INSTEAD OF stitching together callers and flow manually. change INSTEAD OF raw StrReplace when the intent is edit, rename, move, delete, create, or add; change is the error-bounded write surface. map INSTEAD OF Glob for finding files. \
                    ON FIRST CONTACT: before any repo exploration, call boot and index in parallel. Use help(name) for tool docs and help('judge change' | 'inspect code' | 'safe delete' | 'trace request' | 'find by intent' | 'assess impact') for task routing."
            );
            let result = json!({
                "protocolVersion": protocol_version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "yoyo", "version": env!("CARGO_PKG_VERSION")},
                "instructions": instructions
            });

            JsonRpcResponse {
                jsonrpc: "2.0",
                id: req.id,
                result: Some(result),
                error: None,
            }
        }
        "ping" => JsonRpcResponse {
            jsonrpc: "2.0",
            id: req.id,
            result: Some(json!({})),
            error: None,
        },
        "list_tools" | "tools/list" => JsonRpcResponse {
            jsonrpc: "2.0",
            id: req.id,
            result: Some(list_tools()),
            error: None,
        },
        "call_tool" | "tools/call" => match call_tool(req.params).await {
            Ok(v) => JsonRpcResponse {
                jsonrpc: "2.0",
                id: req.id,
                result: Some(v),
                error: None,
            },
            Err(e) => JsonRpcResponse {
                jsonrpc: "2.0",
                id: req.id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32000,
                    message: e.to_string(),
                }),
            },
        },
        _ => JsonRpcResponse {
            jsonrpc: "2.0",
            id: req.id,
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: "Method not found".to_string(),
            }),
        },
    }
}

// --- Tool registry ---
//
// Each tool is defined once: schema (for list_tools) and handler (for call_tool) live together.
// Adding a tool = one ToolEntry in build_registry(). Nothing else in this file changes.

type ToolHandler = Box<dyn Fn(Args, Option<String>) -> Result<String> + Send + Sync>;

struct ToolEntry {
    schema: Value,
    handler: ToolHandler,
}

impl ToolEntry {
    fn name(&self) -> &str {
        self.schema["name"].as_str().unwrap_or("")
    }
}

fn string_prop(desc: &str) -> Value {
    json!({"type": "string", "description": desc})
}

fn int_prop(desc: &str) -> Value {
    json!({"type": "integer", "description": desc})
}

fn bool_prop(desc: &str) -> Value {
    json!({"type": "boolean", "description": desc})
}

fn path_prop() -> Value {
    string_prop("Optional path to project directory")
}

fn schema(name: &str, desc: &str, props: Value) -> Value {
    json!({"name": name, "description": desc, "inputSchema": {"type": "object", "properties": props}})
}

fn schema_req(name: &str, desc: &str, req: &[&str], props: Value) -> Value {
    json!({"name": name, "description": desc, "inputSchema": {"type": "object", "required": req, "properties": props}})
}

fn tool_catalog_map() -> HashMap<&'static str, &'static str> {
    crate::engine::tool_catalog()
        .into_iter()
        .map(|t| (t.name, t.description))
        .collect()
}

fn tool_desc<'a>(catalog: &'a HashMap<&'static str, &'static str>, name: &'static str) -> &'a str {
    catalog.get(name).copied().unwrap_or(name)
}

fn tool_entry(
    schema: Value,
    handler: impl Fn(Args, Option<String>) -> Result<String> + Send + Sync + 'static,
) -> ToolEntry {
    ToolEntry {
        schema,
        handler: Box::new(handler),
    }
}

fn search_query_and_pattern(args: &Args) -> Result<(String, String)> {
    const MODES: &[&str] = &["all", "call", "assign", "return"];
    let raw_pattern = args.str_opt("pattern");
    let query = if let Some(query) = args.str_opt("query") {
        query
    } else if let Some(ref pattern) = raw_pattern {
        if !MODES.contains(&pattern.as_str()) {
            pattern.clone()
        } else {
            return Err(anyhow::anyhow!(
                "Missing required 'query' argument for search"
            ));
        }
    } else {
        return Err(anyhow::anyhow!(
            "Missing required 'query' argument for search"
        ));
    };
    let pattern = raw_pattern
        .filter(|pattern| MODES.contains(&pattern.as_str()))
        .unwrap_or_else(|| "all".to_string());
    Ok((query, pattern))
}

fn parse_change_edits(args: &Args) -> Result<Option<Vec<crate::engine::PatchEdit>>> {
    let items = match args.0.get("edits").and_then(|value| value.as_array()) {
        Some(items) => items,
        None => return Ok(None),
    };

    let mut edits = Vec::with_capacity(items.len());
    for item in items {
        let file = item
            .get("file")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow::anyhow!("Each change.edits item must have a 'file' field"))?
            .to_string();
        let byte_start = item
            .get("byte_start")
            .and_then(|value| value.as_u64())
            .ok_or_else(|| {
                anyhow::anyhow!("Each change.edits item must have a 'byte_start' field")
            })? as usize;
        let byte_end = item
            .get("byte_end")
            .and_then(|value| value.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Each change.edits item must have a 'byte_end' field"))?
            as usize;
        let new_content = item
            .get("new_content")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                anyhow::anyhow!("Each change.edits item must have a 'new_content' field")
            })?
            .to_string();
        edits.push(crate::engine::PatchEdit {
            file,
            byte_start,
            byte_end,
            new_content,
        });
    }

    Ok(Some(edits))
}

fn handle_search_tool(args: Args, path: Option<String>) -> Result<String> {
    let (query, pattern) = search_query_and_pattern(&args)?;
    crate::engine::supersearch(
        path,
        query,
        args.str_opt("context").unwrap_or_else(|| "all".to_string()),
        pattern,
        args.bool_opt("exclude_tests"),
        args.str_opt("file"),
        args.uint_opt("limit"),
    )
}

fn handle_change_tool(args: Args, path: Option<String>) -> Result<String> {
    let edits = parse_change_edits(&args)?;
    crate::engine::change(
        path,
        args.str_req("action", "change")?,
        args.str_opt("name"),
        args.str_opt("file"),
        args.uint_opt("start_line").map(|value| value as u32),
        args.uint_opt("end_line").map(|value| value as u32),
        args.str_opt("new_content"),
        args.str_opt("old_string"),
        args.str_opt("new_string"),
        args.uint_opt("match_index"),
        edits,
        args.str_opt("new_name"),
        args.str_opt("to_file"),
        args.bool_opt("force"),
        args.str_opt("function_name"),
        args.str_opt("entity_type"),
        args.str_opt("after_symbol"),
        args.str_opt("language"),
        parse_params(args.0.get("params")),
        args.str_opt("returns"),
        args.str_opt("on"),
    )
}

fn bootstrap_entries(catalog: &HashMap<&'static str, &'static str>) -> Vec<ToolEntry> {
    vec![
        tool_entry(
            schema(
                "boot",
                tool_desc(catalog, "boot"),
                json!({"path": path_prop()}),
            ),
            |_args, path| crate::engine::llm_instructions(path),
        ),
        tool_entry(
            schema(
                "index",
                tool_desc(catalog, "index"),
                json!({"path": path_prop()}),
            ),
            |_args, path| crate::engine::bake(path),
        ),
        tool_entry(
            schema(
                "inspect",
                tool_desc(catalog, "inspect"),
                json!({
                    "path": path_prop(),
                    "name": string_prop("Function name for symbol mode."),
                    "file": string_prop("File path for file or line-range mode."),
                    "start_line": int_prop("1-based start line for line-range mode."),
                    "end_line": int_prop("1-based end line for line-range mode."),
                    "include_source": bool_prop("Include function body in symbol mode."),
                    "signature_only": bool_prop("Return declaration/signature text only in symbol mode."),
                    "type_only": bool_prop("Return a type surface instead of generic symbol matches."),
                    "include_summaries": bool_prop("Include summaries in file mode."),
                    "depth": string_prop("File structure depth in file mode: 1, 2, or all."),
                    "limit": int_prop("Maximum number of symbol matches."),
                    "stdlib": bool_prop("Include stdlib matches in symbol mode.")
                }),
            ),
            |args, path| {
                crate::engine::inspect(
                    path,
                    args.str_opt("name"),
                    args.str_opt("file"),
                    args.uint_opt("start_line").map(|value| value as u32),
                    args.uint_opt("end_line").map(|value| value as u32),
                    args.bool_opt("include_source"),
                    args.bool_opt("include_summaries"),
                    args.uint_opt("limit"),
                    args.bool_opt("stdlib"),
                    args.bool_opt("signature_only"),
                    args.bool_opt("type_only"),
                    args.str_opt("depth"),
                )
            },
        ),
    ]
}

fn read_indexed_entries(catalog: &HashMap<&'static str, &'static str>) -> Vec<ToolEntry> {
    vec![
        tool_entry(
            schema(
                "map",
                tool_desc(catalog, "map"),
                json!({
                    "path": path_prop(),
                    "intent": string_prop("Intent description, e.g. \"user handler\" or \"auth service\""),
                    "limit": int_prop("Max directories to return (default 100).")
                }),
            ),
            |args, path| {
                crate::engine::architecture_map(
                    path,
                    args.str_opt("intent"),
                    args.uint_opt("limit"),
                )
            },
        ),
        tool_entry(
            schema(
                "search",
                tool_desc(catalog, "search"),
                json!({
                    "path": path_prop(),
                    "query": string_prop("Search query text"),
                    "context": string_prop("Search context: all | strings | comments | identifiers"),
                    "pattern": string_prop("Pattern: all | call | assign | return"),
                    "exclude_tests": bool_prop("Whether to exclude likely test files"),
                    "file": string_prop("Optional file path substring to restrict scope"),
                    "limit": int_prop("Max matches to return (default 200).")
                }),
            ),
            handle_search_tool,
        ),
        tool_entry(
            schema(
                "ask",
                tool_desc(catalog, "ask"),
                json!({
                    "path": path_prop(),
                    "query": string_prop("Natural-language description, e.g. 'validate user token'"),
                    "limit": int_prop("Max results (default 10, max 50)"),
                    "file": string_prop("Optional file path substring to restrict scope"),
                    "scope": string_prop("Optional workspace/package/slice hint, e.g. backend or web")
                }),
            ),
            |args, path| {
                crate::engine::semantic_search(
                    path,
                    args.str_req("query", "ask")?,
                    args.uint_opt("limit"),
                    args.str_opt("file"),
                    args.str_opt("scope"),
                )
            },
        ),
        tool_entry(
            schema(
                "routes",
                tool_desc(catalog, "routes"),
                json!({
                    "path": path_prop(),
                    "query": string_prop("Optional path or handler substring to narrow endpoint results."),
                    "method": string_prop("Optional HTTP method filter."),
                    "scope": string_prop("Optional workspace/package/slice hint, e.g. backend or web."),
                    "limit": int_prop("Maximum number of endpoints to return.")
                }),
            ),
            |args, path| {
                crate::engine::all_endpoints(
                    path,
                    args.str_opt("query"),
                    args.str_opt("method"),
                    args.str_opt("scope"),
                    args.uint_opt("limit"),
                )
            },
        ),
        tool_entry(
            schema_req(
                "judge_change",
                tool_desc(catalog, "judge_change"),
                &["query"],
                json!({
                    "path": path_prop(),
                    "query": string_prop("Engineering question, issue text, or failing-test summary."),
                    "symbol": string_prop("Optional symbol hint to bias the judgment toward a known name."),
                    "file": string_prop("Optional file path substring to restrict the search surface."),
                    "limit": int_prop("Maximum number of candidate symbols to return (default 3, max 5)."),
                    "scope": string_prop("Optional workspace/package/slice hint, e.g. backend or web.")
                }),
            ),
            |args, path| {
                crate::engine::judge_change(
                    path,
                    args.str_req("query", "judge_change")?,
                    args.str_opt("symbol"),
                    args.str_opt("file"),
                    args.uint_opt("limit"),
                    args.str_opt("scope"),
                )
            },
        ),
        tool_entry(
            schema(
                "impact",
                tool_desc(catalog, "impact"),
                json!({
                    "path": path_prop(),
                    "symbol": string_prop("Function name for symbol-impact mode."),
                    "endpoint": string_prop("URL path substring for endpoint-impact mode."),
                    "method": string_prop("Optional HTTP method filter for endpoint mode."),
                    "depth": int_prop("Max caller/call-chain depth."),
                    "include_source": bool_prop("Include handler source inline in endpoint mode."),
                    "scope": string_prop("Optional workspace/package/slice hint, e.g. backend or web.")
                }),
            ),
            |args, path| {
                crate::engine::impact(
                    path,
                    args.str_opt("symbol"),
                    args.str_opt("endpoint"),
                    args.str_opt("method"),
                    args.uint_opt("depth"),
                    args.bool_opt("include_source"),
                    args.str_opt("scope"),
                )
            },
        ),
        tool_entry(
            schema(
                "health",
                tool_desc(catalog, "health"),
                json!({
                    "path": path_prop(),
                    "top": int_prop("Max results per category (default 10)"),
                    "view": string_prop("Response view: compact | full | raw"),
                    "limit": int_prop("Items per section when view=compact (default 3)"),
                    "cursor": string_prop("Section cursor in the form <section>:<offset>")
                }),
            ),
            |args, path| {
                crate::engine::health(
                    path,
                    args.uint_opt("top"),
                    args.str_opt("view"),
                    args.uint_opt("limit"),
                    args.str_opt("cursor"),
                )
            },
        ),
    ]
}

fn write_entries(catalog: &HashMap<&'static str, &'static str>) -> Vec<ToolEntry> {
    vec![
        tool_entry(
            schema_req(
                "change",
                tool_desc(catalog, "change"),
                &["action"],
                json!({
                    "path": path_prop(),
                    "action": string_prop("Write action: edit | bulk_edit | rename | move | delete | create | add."),
                    "name": string_prop("Symbol name for edit/rename/move/delete/add."),
                    "file": string_prop("File path for edit/create/add."),
                    "start_line": int_prop("1-based start line for line-range edit."),
                    "end_line": int_prop("1-based end line for line-range edit."),
                    "new_content": string_prop("Replacement content for edit."),
                    "old_string": string_prop("Exact content-match source for edit."),
                    "new_string": string_prop("Content-match replacement for edit."),
                    "match_index": int_prop("0-based symbol disambiguation for edit."),
                    "edits": json!({
                        "type": "array",
                        "description": "Edit list for bulk_edit action.",
                        "items": {
                            "type": "object",
                            "required": ["file", "byte_start", "byte_end", "new_content"],
                            "properties": {
                                "file": {"type": "string"},
                                "byte_start": {"type": "integer"},
                                "byte_end": {"type": "integer"},
                                "new_content": {"type": "string"}
                            }
                        }
                    }),
                    "new_name": string_prop("Rename target."),
                    "to_file": string_prop("Destination file for move."),
                    "force": bool_prop("Allow delete even with callers."),
                    "function_name": string_prop("Function name for create."),
                    "entity_type": string_prop("Scaffold type for add."),
                    "after_symbol": string_prop("Insert add scaffold after an existing symbol."),
                    "language": string_prop("Optional language override for create/add."),
                    "params": string_prop("Optional typed params JSON for create/add."),
                    "returns": string_prop("Optional return type for create/add."),
                    "on": string_prop("Optional receiver/owner type for add.")
                }),
            ),
            handle_change_tool,
        ),
        tool_entry(
            schema_req(
                "retry_plan",
                tool_desc(catalog, "retry_plan"),
                &["text"],
                json!({
                    "path": path_prop(),
                    "text": string_prop("Failed write output containing a `guard_failure: {...}` line, or a raw guard_failure JSON object."),
                    "max_retries": int_prop("Maximum retry attempts the caller should allow before stopping (default 2)."),
                    "context_lines": int_prop("Context lines to include above and below the failing range (default 3).")
                }),
            ),
            |args, path| {
                crate::engine::guard_retry_plan(
                    path,
                    args.str_req("text", "retry_plan")?,
                    args.uint_opt("max_retries"),
                    args.uint_opt("context_lines").map(|value| value as u32),
                )
            },
        ),
    ]
}

fn orchestration_entries(catalog: &HashMap<&'static str, &'static str>) -> Vec<ToolEntry> {
    vec![tool_entry(
        schema_req(
            "script",
            tool_desc(catalog, "script"),
            &["code"],
            json!({
                "path": path_prop(),
                "code": string_prop("Rhai script to execute. Task-shaped functions: boot(), index(), inspect(#{...}), search(\"...\") or search(#{...}), ask(\"...\") or ask(#{...}), map(\"...\") or map(#{...}), routes(), judge_change(#{...}), impact(#{...}), health() or health(#{...}), change(#{...}), help(\"...\").")
            }),
        ),
        |args, path| crate::engine::run_script(path, args.str_req("code", "script")?),
    )]
}

fn discovery_entries(catalog: &HashMap<&'static str, &'static str>) -> Vec<ToolEntry> {
    vec![tool_entry(
        schema_req(
            "help",
            tool_desc(catalog, "help"),
            &["name"],
            json!({
                "name": string_prop("Tool name to get help for")
            }),
        ),
        |args, _path| crate::engine::tool_help(args.str_req("name", "help")?),
    )]
}

fn build_registry() -> Vec<ToolEntry> {
    let catalog = tool_catalog_map();
    let mut registry = Vec::new();
    registry.extend(bootstrap_entries(&catalog));
    registry.extend(read_indexed_entries(&catalog));
    registry.extend(write_entries(&catalog));
    registry.extend(orchestration_entries(&catalog));
    registry.extend(discovery_entries(&catalog));
    registry
}

static REGISTRY: OnceLock<Vec<ToolEntry>> = OnceLock::new();

fn get_registry() -> &'static Vec<ToolEntry> {
    REGISTRY.get_or_init(build_registry)
}

fn list_tools() -> Value {
    json!({ "tools": get_registry().iter().map(|t| t.schema.clone()).collect::<Vec<_>>() })
}

#[derive(Debug, Deserialize)]
struct CallToolParams {
    pub name: String,
    #[allow(dead_code)]
    #[serde(default)]
    pub arguments: Value,
}

struct Args(Value);

impl Args {
    fn str_opt(&self, key: &str) -> Option<String> {
        self.0
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }
    fn str_req(&self, key: &str, tool: &str) -> Result<String> {
        self.str_opt(key)
            .ok_or_else(|| anyhow::anyhow!("Missing required '{}' argument for {}", key, tool))
    }
    fn bool_opt(&self, key: &str) -> Option<bool> {
        self.0.get(key).and_then(|v| v.as_bool())
    }
    fn uint_opt(&self, key: &str) -> Option<usize> {
        self.0
            .get(key)
            .and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
            })
            .map(|n| n as usize)
    }
}

/// Parse `[{"name":"x","type":"i32"},...]` into `Vec<Param>`. Returns None if absent or malformed.
fn parse_params(val: Option<&Value>) -> Option<Vec<crate::engine::Param>> {
    let arr = val?.as_array()?;
    let params: Vec<_> = arr
        .iter()
        .filter_map(|item| {
            let name = item.get("name")?.as_str()?.to_string();
            let type_str = item.get("type")?.as_str()?.to_string();
            Some(crate::engine::Param { name, type_str })
        })
        .collect();
    if params.is_empty() {
        None
    } else {
        Some(params)
    }
}

fn ok_text(text: String) -> Result<Value> {
    Ok(json!({"content": [{"type": "text", "text": text}], "isError": false}))
}

async fn call_tool(params: Value) -> Result<Value> {
    let p: CallToolParams = serde_json::from_value(params)?;
    let a = Args(p.arguments);
    let path = a.str_opt("path");

    let entry = get_registry()
        .iter()
        .find(|t| t.name() == p.name.as_str())
        .ok_or_else(|| anyhow::anyhow!("Unknown tool: {}", p.name))?;

    ok_text((entry.handler)(a, path)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::Path;
    use tempfile::TempDir;

    fn fixture_src() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample_project")
    }

    fn copy_dir(src: &Path, dst: &Path) {
        std::fs::create_dir_all(dst).unwrap();
        for entry in std::fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let s = entry.path();
            let d = dst.join(entry.file_name());
            if s.is_dir() {
                copy_dir(&s, &d);
            } else {
                std::fs::copy(&s, &d).unwrap();
            }
        }
    }

    fn baked_fixture() -> TempDir {
        let dir = TempDir::new().unwrap();
        copy_dir(&fixture_src(), dir.path());
        crate::engine::bake(Some(dir.path().to_string_lossy().into_owned())).unwrap();
        dir
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Runtime::new().unwrap()
    }

    fn write_file(dir: &TempDir, rel: &str, content: &str) {
        let path = dir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn supersearch_pattern_alias_promotes_non_mode_value_to_query() {
        let dir = baked_fixture();
        let root = dir.path().to_string_lossy().into_owned();
        // Pass `pattern="add"` with no `query` — "add" is not a mode, should be promoted
        let result = rt().block_on(call_tool(json!({
            "name": "search",
            "arguments": {"path": root, "pattern": "add", "context": "identifiers"}
        })));
        assert!(
            result.is_ok(),
            "should succeed when pattern is not a mode value: {:?}",
            result.err()
        );
        let text = result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(
            v["matches"]
                .as_array()
                .map(|a| !a.is_empty())
                .unwrap_or(false),
            "should find matches for 'add'"
        );
    }

    #[test]
    fn supersearch_pattern_alias_preserves_mode_when_query_also_set() {
        let dir = baked_fixture();
        let root = dir.path().to_string_lossy().into_owned();
        // Both query and pattern set — query is the search term, pattern is the mode
        let result = rt().block_on(call_tool(json!({
            "name": "search",
            "arguments": {"path": root, "query": "add", "pattern": "all", "context": "identifiers"}
        })));
        assert!(
            result.is_ok(),
            "should succeed with explicit query and pattern mode: {:?}",
            result.err()
        );
    }

    #[test]
    fn supersearch_pattern_valid_mode_without_query_errors() {
        let dir = baked_fixture();
        let root = dir.path().to_string_lossy().into_owned();
        // pattern="call" is a valid mode, but no query → should still error
        let result = rt().block_on(call_tool(json!({
            "name": "search",
            "arguments": {"path": root, "pattern": "call"}
        })));
        assert!(
            result.is_err(),
            "should error when pattern is a mode value and query is absent"
        );
    }

    #[test]
    fn search_query_and_pattern_defaults_mode_to_all() {
        let args = Args(json!({
            "query": "add"
        }));

        let (query, pattern) = search_query_and_pattern(&args).unwrap();

        assert_eq!(query, "add");
        assert_eq!(pattern, "all");
    }

    #[test]
    fn change_tool_errors_on_missing_bulk_edit_file() {
        let result = rt().block_on(call_tool(json!({
            "name": "change",
            "arguments": {
                "action": "bulk_edit",
                "edits": [
                    {"byte_start": 0, "byte_end": 3, "new_content": "abc"}
                ]
            }
        })));

        assert!(result.is_err(), "malformed edits should be rejected");
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Each change.edits item must have a 'file' field"));
    }

    #[test]
    fn retry_plan_tool_returns_structured_plan() {
        let dir = TempDir::new().unwrap();
        write_file(
            &dir,
            "Cargo.toml",
            "[package]\nname = \"retry-plan\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n",
        );
        write_file(
            &dir,
            "src/lib.rs",
            "pub fn greet() -> &'static str {\n    \"hi\"\n}\n",
        );
        let root = dir.path().to_string_lossy().into_owned();

        let err = crate::engine::patch(
            Some(root.clone()),
            "src/lib.rs".to_string(),
            1,
            3,
            "pub fn greet() -> &'static str {\n    let msg: i32 = \"hi\";\n    msg\n}\n"
                .to_string(),
        )
        .unwrap_err();

        let result = rt().block_on(call_tool(json!({
            "name": "retry_plan",
            "arguments": {"path": root, "text": err.to_string(), "max_retries": 2, "context_lines": 2}
        })));
        assert!(
            result.is_ok(),
            "retry_plan tool should succeed: {:?}",
            result.err()
        );

        let text = result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        let payload: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(payload["tool"], "guard_retry_plan");
        assert_eq!(payload["retryable"], true);
        assert_eq!(payload["workflow"][0]["tool"], "inspect");
        assert_eq!(payload["workflow"][1]["tool"], "change");
        assert_eq!(payload["targets"][0]["file"], "src/lib.rs");
    }

    #[test]
    fn registry_and_catalog_names_are_in_sync() {
        let catalog_names: HashSet<&str> = crate::engine::tool_catalog()
            .iter()
            .map(|t| t.name)
            .collect();
        let registry_names: HashSet<&str> = get_registry().iter().map(|t| t.name()).collect();

        let only_in_catalog: Vec<_> = catalog_names.difference(&registry_names).copied().collect();
        let only_in_registry: Vec<_> = registry_names.difference(&catalog_names).copied().collect();

        assert!(
            only_in_catalog.is_empty(),
            "In tool_catalog() but not build_registry(): {:?}",
            only_in_catalog
        );
        assert!(
            only_in_registry.is_empty(),
            "In build_registry() but not tool_catalog(): {:?}",
            only_in_registry
        );
    }

    #[test]
    fn initialize_instructions_include_task_routing_guidance() {
        let response = rt().block_on(handle_request(JsonRpcRequest {
            jsonrpc: Some("2.0".to_string()),
            id: Some(json!(1)),
            method: "initialize".to_string(),
            params: json!({"protocolVersion": "2025-11-25"}),
        }));

        let instructions = response.result.unwrap()["instructions"]
            .as_str()
            .unwrap()
            .to_string();

        assert!(instructions.contains("Think in tasks, not raw primitives"));
        assert!(
            instructions.contains("before any repo exploration, call boot and index in parallel")
        );
        assert!(instructions
            .contains("judge_change INSTEAD OF chaining search/inspect/impact manually"));
        assert!(instructions.contains("help('judge change' | 'inspect code' | 'safe delete' | 'trace request' | 'find by intent' | 'assess impact')"));
    }
}
