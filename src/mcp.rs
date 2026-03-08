use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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

            let result = json!({
                "protocolVersion": protocol_version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "yoyo", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "You have access to yoyo, a code intelligence MCP server — 27 tools to read and edit any codebase from the AST, not model memory. \
                    ON FIRST CONTACT: call `llm_instructions` and `bake` in parallel — do not wait for one before starting the other. \
                    `llm_instructions` returns the full tool catalog, 21 combination workflows, prime directives, and antipatterns. Read it before doing anything else. \
                    `bake` builds the index all read-indexed tools depend on. \
                    THE COMBINATIONS ARE THE POINT: no single tool is impressive — the chains are. \
                    Key combos: health→blast_radius→graph_delete (safe dead code removal), flow→symbol→multi_patch (fix endpoint end-to-end), blast_radius→graph_rename→symbol (safe rename). \
                    REPLACEMENTS — no exceptions: supersearch replaces grep/rg. symbol+include_source replaces cat/Read. slice replaces line-range reads. patch replaces Edit for function-level changes. flow replaces api_trace+trace_down+symbol."
            });

            JsonRpcResponse { jsonrpc: "2.0", id: req.id, result: Some(result), error: None }
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
            Ok(v) => JsonRpcResponse { jsonrpc: "2.0", id: req.id, result: Some(v), error: None },
            Err(e) => JsonRpcResponse {
                jsonrpc: "2.0",
                id: req.id,
                result: None,
                error: Some(JsonRpcError { code: -32000, message: e.to_string() }),
            },
        },
        _ => JsonRpcResponse {
            jsonrpc: "2.0",
            id: req.id,
            result: None,
            error: Some(JsonRpcError { code: -32601, message: "Method not found".to_string() }),
        },
    }
}

fn list_tools() -> Value {
    fn s(desc: &str) -> Value { json!({"type": "string", "description": desc}) }
    fn i(desc: &str) -> Value { json!({"type": "integer", "description": desc}) }
    fn b(desc: &str) -> Value { json!({"type": "boolean", "description": desc}) }
    fn p() -> Value { s("Optional path to project directory") }
    fn tool(name: &str, desc: &str, props: Value) -> Value {
        json!({"name": name, "description": desc, "inputSchema": {"type": "object", "properties": props}})
    }
    fn tool_req(name: &str, desc: &str, req: &[&str], props: Value) -> Value {
        json!({"name": name, "description": desc, "inputSchema": {"type": "object", "required": req, "properties": props}})
    }

    json!({ "tools": [
        tool("llm_instructions", "Bootstrap: full tool catalog, 21 combination workflows, prime directives, and antipatterns. Call in parallel with bake on first contact — do not skip.", json!({"path": p()})),
        tool("shake", "30-second codebase overview: language breakdown, file count, top-complexity functions. Use first when orienting to an unfamiliar project. Pair with architecture_map for full orientation.", json!({"path": p()})),
        tool("bake", "Build the AST index all read-indexed tools depend on. Call in parallel with llm_instructions on first contact. Re-run after large external changes (git pull, generated files).", json!({"path": p()})),
        tool("symbol", "Find a function by name — returns file, line range, visibility, calls, and optionally full source. Always pass include_source=true when you need to read the body; never use Read/cat instead. Use file to scope when names collide across modules.", json!({
            "path": p(),
            "name": s("Symbol (function) name to look up"),
            "include_source": b("If true, include the function body (source code) in each match"),
            "file": s("Optional file path substring to narrow results (e.g. 'routes/user' or 'tcp_core')"),
            "limit": i("Max matches to return (default 20). Lower when include_source=true to stay within context limits.")
        })),
        tool("all_endpoints", "List all detected HTTP routes. Use when flow returns no match — find the exact path substring here, then retry flow. Supports Express, Actix, Flask, FastAPI, gin, echo.", json!({"path": p()})),
        tool_req("flow", "One-call vertical slice: endpoint → handler → call chain to db/http/queue boundary. Always prefer over api_trace+trace_down+symbol — those three tools combined do less. Pair with multi_patch to fix the full chain in one session.", &["endpoint"], json!({
            "path": p(),
            "endpoint": s("URL path substring to match (e.g. '/users' or '/api/login')"),
            "method": s("Optional HTTP method filter (GET, POST, PUT, DELETE, PATCH)"),
            "depth": i("Max call chain depth (default 5)"),
            "include_source": b("If true, include the handler function source inline")
        })),
        tool("slice", "Read any line range from any file. Use start_line/end_line from symbol output directly — no arithmetic needed. Prefer over Read/cat for targeted reads; use symbol+include_source for full function bodies.", json!({
            "path": p(),
            "file": s("File path relative to the project root"),
            "start_line": i("1-based start line (inclusive). Matches the start_line field from symbol output."),
            "end_line": i("1-based end line (inclusive). Matches the end_line field from symbol output.")
        })),
        tool("api_surface", "All exported functions grouped by module — understand the public contract without reading files. Use during orientation alongside shake and architecture_map.", json!({
            "path": p(),
            "package": s("Optional package/module filter (substring match on module or file paths)"),
            "limit": i("Maximum number of functions per module (default 20)")
        })),
        tool("file_functions", "Every function in a file with line ranges and cyclomatic complexity. Use after package_summary to drill into a specific file. Complexity scores flag candidates for refactoring.", json!({
            "path": p(),
            "file": s("File path relative to the project root"),
            "include_summaries": b("Whether to include summaries (currently a no-op placeholder)")
        })),
        tool("supersearch", "AST-aware search — replaces grep/rg entirely, do not use grep. Use context=identifiers+pattern=call for call-site search. Pair with symbol+include_source to read matches in full context. Use file to restrict scope on large codebases.", json!({
            "path": p(),
            "query": s("Search query text"),
            "context": s("Search context: all | strings | comments | identifiers"),
            "pattern": s("Pattern: all | call | assign | return"),
            "exclude_tests": b("Whether to exclude likely test files"),
            "file": s("Optional file path substring to restrict scope (e.g. 'src/routes' or 'tcp')"),
            "limit": i("Max matches to return (default 200). Reduce for large codebases with common terms.")
        })),
        tool("package_summary", "All functions, endpoints, and complexity for a module path substring. Use before file_functions when you don't know which file to drill into yet.", json!({
            "path": p(),
            "package": s("Package/module name or directory substring")
        })),
        tool("architecture_map", "Directory tree with inferred roles (routes, services, models, utils). Use at session start when orienting to a new codebase. Pass intent to get placement hints for a new feature.", json!({
            "path": p(),
            "intent": s("Intent description, e.g. \"user handler\" or \"auth service\"")
        })),
        tool("suggest_placement", "Ranked file suggestions for where to add a new function, based on related symbols. Use after architecture_map and before graph_create/graph_add.", json!({
            "path": p(),
            "function_name": s("Name of the function to add"),
            "function_type": s("Function type: handler | service | repository | model | util | test"),
            "related_to": s("Existing related symbol or substring (optional)")
        })),
        tool("crud_operations", "Create/read/update/delete matrix per entity inferred from routes. Use to understand data flow before modifying endpoints. Pair with api_trace to drill into a specific operation.", json!({
            "path": p(),
            "entity": s("Optional entity filter (e.g. \"user\")")
        })),
        tool("api_trace", "Resolve a route path+method to its handler function. Prefer flow over this — flow does api_trace+trace_down+symbol in one call. Use api_trace only when you need the handler name without the full chain.", json!({
            "path": p(),
            "endpoint": s("Endpoint path (or substring), e.g. \"/users\""),
            "method": s("Optional HTTP method (GET, POST, etc.)")
        })),
        tool("find_docs", "Locate README, .env, Dockerfile, and config files. Use at session start when you need project context. Pair with slice to read the first N lines of any matched file.", json!({
            "path": p(),
            "doc_type": s("Documentation type: readme | env | config | docker | all")
        })),
        tool("patch", "Write changes to a file. Three modes: name mode (pass name+new_content — safest for full function rewrites), line-range mode (file+start+end+new_content), content-match mode (file+old_string+new_string — immune to line drift, preferred for partial edits). Always read with symbol+include_source first.", json!({
            "path": p(),
            "name": s("Symbol name to patch (resolves location from bake index). Use with new_content; optional match_index when multiple matches."),
            "match_index": i("0-based index when multiple symbols match name (default 0)"),
            "file": s("File path relative to project root (for range-based or content-match patch)"),
            "start": i("1-based start line (inclusive), for range-based patch"),
            "end": i("1-based end line (inclusive), for range-based patch"),
            "new_content": s("Replacement content for range-based patch"),
            "old_string": s("Exact string to find and replace (content-match mode — immune to line drift)"),
            "new_string": s("Replacement string for content-match mode")
        })),
        tool_req("patch_bytes", "Splice at exact byte offsets — use byte_start/byte_end from the bake index. For single-identifier replacements where name/content-match modes would affect too much. Prefer patch for function-level edits.", &["file", "byte_start", "byte_end", "new_content"], json!({
            "path": p(),
            "file": s("File path relative to project root"),
            "byte_start": i("Inclusive start byte offset"),
            "byte_end": i("Exclusive end byte offset"),
            "new_content": s("Replacement text")
        })),
        tool_req("multi_patch", "Apply N edits across M files in one call — bottom-up ordering is automatic so offsets stay valid. Use after flow to fix an entire call chain end-to-end, or after blast_radius to update all callers. Prefer graph_rename for pure renames.", &["edits"], json!({
            "path": p(),
            "edits": json!({
                "type": "array",
                "description": "Array of edit operations",
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
            })
        })),
        tool_req("blast_radius", "All transitive callers of a symbol + affected files. Always run before graph_delete or graph_rename — skip this and you risk breaking callers silently. Prefer over grep for caller discovery: grep overcounts by hitting comments, strings, and partial name matches.", &["symbol"], json!({
            "path": p(),
            "symbol": s("Function name to analyse (exact match on the callee name)"),
            "depth": i("Maximum call-graph depth to traverse (default 2)")
        })),
        tool_req("graph_rename", "Rename a symbol at its definition and every call site atomically. Word-boundary matching prevents partial renames (renaming 'parse' won't corrupt 'parse_all'). Always prefer over str.replace or multi_patch for renames. Run blast_radius first to understand scope.", &["name", "new_name"], json!({
            "path": p(),
            "name": s("Current identifier name to rename"),
            "new_name": s("New identifier name")
        })),
        tool_req("graph_create", "Create a new file with an initial function scaffold and auto-reindex. Errors if file exists or parent dir is missing — check first with find_docs or architecture_map. Use graph_add instead when adding to an existing file.", &["file", "function_name"], json!({
            "path": p(),
            "file": s("File path relative to project root (e.g. 'src/engine/foo.rs')"),
            "function_name": s("Name for the initial scaffolded function"),
            "language": s("Optional: override language detection (rust | typescript | python | go | java | c | cpp)")
        })),
        tool_req("graph_add", "Insert a function scaffold into an existing file, optionally after a named symbol. Auto-reindexes. Use graph_create for new files. Pair with patch to fill in the scaffold body immediately after.", &["entity_type", "name", "file"], json!({
            "path": p(),
            "entity_type": s("Scaffold type: fn (Rust) | function (TS/JS) | def (Python) | func (Go)"),
            "name": s("Name for the new function/entity"),
            "file": s("File path relative to project root"),
            "after_symbol": s("Optional: insert after this existing symbol (name or substring)"),
            "language": s("Optional: override language detection (rust | typescript | python | go)")
        })),
        tool_req("graph_move", "Move a function between files atomically — removes from source, appends to destination, reindexes both. Run bake first to ensure byte offsets are fresh. Check blast_radius to understand import impact before moving.", &["name", "to_file"], json!({
            "path": p(),
            "name": s("Exact function name to move (matched case-insensitively in bake index)"),
            "to_file": s("Destination file path relative to project root")
        })),
        tool_req("trace_down", "BFS call chain from a function to db/http/queue boundaries. Rust and Go only. Prefer flow for endpoint tracing — flow calls trace_down internally and returns more context. Use trace_down directly for non-endpoint functions.", &["name"], json!({
            "path": p(),
            "name": s("Function name to start the trace from"),
            "depth": i("Maximum call depth to follow (default 5)"),
            "file": s("Optional file path substring to disambiguate when multiple functions share the same name")
        })),
        tool("semantic_search", "Find functions by intent when you don't know the name. Local ONNX embeddings — no API key, no external calls. Pair with symbol+include_source to read top matches. Use when supersearch finds nothing (supersearch needs a name/pattern, semantic_search needs a description).", json!({
            "path": p(),
            "query": s("Natural-language description, e.g. 'validate user token' or 'send email notification'"),
            "limit": i("Max results (default 10, max 50)"),
            "file": s("Optional file path substring to restrict scope")
        })),
        tool("health", "Dead code, god functions, and duplicate name hints. Gotcha: router-registered handlers may appear as dead code — cross-check with blast_radius before deleting. Use as first step of the safe-delete combo: health→blast_radius→graph_delete.", json!({
            "path": p(),
            "top": i("Max results per category (default 10)")
        })),
        tool_req("graph_delete", "Remove a function by name. Blocks if callers exist — this is a safety net, not an error. Always run health→blast_radius first to confirm the function is truly dead. Use force=true only when you have verified callers are intentional (e.g. test-only).", &["name"], json!({
            "path": p(),
            "name": s("Exact function name to delete (matched case-insensitively in bake index)"),
            "file": s("Optional file path substring to disambiguate when multiple functions share the same name"),
            "force": b("Delete even if active callers exist (default false)")
        })),
    ]})
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
        self.0.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
    }
    fn str_req(&self, key: &str, tool: &str) -> Result<String> {
        self.str_opt(key)
            .ok_or_else(|| anyhow::anyhow!("Missing required '{}' argument for {}", key, tool))
    }
    fn bool_opt(&self, key: &str) -> Option<bool> {
        self.0.get(key).and_then(|v| v.as_bool())
    }
    fn uint_opt(&self, key: &str) -> Option<usize> {
        self.0.get(key).and_then(|v| v.as_u64()).map(|n| n as usize)
    }
    fn uint_req(&self, key: &str, tool: &str) -> Result<u64> {
        self.0.get(key).and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing required '{}' argument for {}", key, tool))
    }
}

fn ok_text(text: String) -> Result<Value> {
    Ok(json!({"content": [{"type": "text", "text": text}], "isError": false}))
}

async fn call_tool(params: Value) -> Result<Value> {
    let p: CallToolParams = serde_json::from_value(params)?;
    let a = Args(p.arguments);
    let path = a.str_opt("path");

    match p.name.as_str() {
        "llm_instructions" => ok_text(crate::engine::llm_instructions(path)?),
        "shake"            => ok_text(crate::engine::shake(path)?),
        "bake"             => ok_text(crate::engine::bake(path)?),
        "all_endpoints"    => ok_text(crate::engine::all_endpoints(path)?),
        "flow" => ok_text(crate::engine::flow(
            path,
            a.str_req("endpoint", "flow")?,
            a.str_opt("method"),
            a.uint_opt("depth"),
            a.bool_opt("include_source").unwrap_or(false),
        )?),
        "symbol" => ok_text(crate::engine::symbol(
            path,
            a.str_req("name", "symbol")?,
            a.bool_opt("include_source").unwrap_or(false),
            a.str_opt("file"),
            a.uint_opt("limit"),
        )?),
        "slice" => ok_text(crate::engine::slice(
            path,
            a.str_req("file", "slice")?,
            a.uint_req("start_line", "slice")? as u32,
            a.uint_req("end_line", "slice")? as u32,
        )?),
        "api_surface" => ok_text(crate::engine::api_surface(
            path, a.str_opt("package"), a.uint_opt("limit"),
        )?),
        "file_functions" => ok_text(crate::engine::file_functions(
            path, a.str_req("file", "file_functions")?, a.bool_opt("include_summaries"),
        )?),
        "supersearch" => ok_text(crate::engine::supersearch(
            path,
            a.str_req("query", "supersearch")?,
            a.str_opt("context").unwrap_or_else(|| "all".to_string()),
            a.str_opt("pattern").unwrap_or_else(|| "all".to_string()),
            a.bool_opt("exclude_tests"),
            a.str_opt("file"),
            a.uint_opt("limit"),
        )?),
        "package_summary"  => ok_text(crate::engine::package_summary(path, a.str_req("package", "package_summary")?)?),
        "architecture_map" => ok_text(crate::engine::architecture_map(path, a.str_opt("intent"))?),
        "suggest_placement" => ok_text(crate::engine::suggest_placement(
            path,
            a.str_req("function_name", "suggest_placement")?,
            a.str_req("function_type", "suggest_placement")?,
            a.str_opt("related_to"),
        )?),
        "crud_operations" => ok_text(crate::engine::crud_operations(path, a.str_opt("entity"))?),
        "api_trace" => ok_text(crate::engine::api_trace(
            path, a.str_req("endpoint", "api_trace")?, a.str_opt("method"),
        )?),
        "find_docs" => ok_text(crate::engine::find_docs(
            path, a.str_req("doc_type", "find_docs")?, a.uint_opt("limit"),
        )?),
        "patch" => {
            if let Some(old_string) = a.str_opt("old_string") {
                let new_string = a.str_req("new_string", "patch")?;
                ok_text(crate::engine::patch_string(path, a.str_req("file", "patch")?, old_string, new_string)?)
            } else {
                let new_content = a.str_req("new_content", "patch")?;
                let json = if let Some(name) = a.str_opt("name") {
                    crate::engine::patch_by_symbol(path, name, new_content, a.uint_opt("match_index"))?
                } else {
                    crate::engine::patch(
                        path,
                        a.str_req("file", "patch")?,
                        a.uint_req("start", "patch")? as u32,
                        a.uint_req("end", "patch")? as u32,
                        new_content,
                    )?
                };
                ok_text(json)
            }
        }
        "patch_bytes" => ok_text(crate::engine::patch_bytes(
            path,
            a.str_req("file", "patch_bytes")?,
            a.uint_req("byte_start", "patch_bytes")? as usize,
            a.uint_req("byte_end", "patch_bytes")? as usize,
            a.str_req("new_content", "patch_bytes")?,
        )?),
        "multi_patch" => {
            let edits_val = a.0.get("edits").and_then(|v| v.as_array())
                .ok_or_else(|| anyhow::anyhow!("Missing required 'edits' argument for multi_patch"))?;
            let mut edits = Vec::new();
            for item in edits_val {
                let file = item.get("file").and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Each edit must have a 'file' field"))?.to_string();
                let byte_start = item.get("byte_start").and_then(|v| v.as_u64())
                    .ok_or_else(|| anyhow::anyhow!("Each edit must have a 'byte_start' field"))? as usize;
                let byte_end = item.get("byte_end").and_then(|v| v.as_u64())
                    .ok_or_else(|| anyhow::anyhow!("Each edit must have a 'byte_end' field"))? as usize;
                let new_content = item.get("new_content").and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Each edit must have a 'new_content' field"))?.to_string();
                edits.push(crate::engine::PatchEdit { file, byte_start, byte_end, new_content });
            }
            ok_text(crate::engine::multi_patch(path, edits)?)
        }
        "blast_radius" => ok_text(crate::engine::blast_radius(
            path, a.str_req("symbol", "blast_radius")?, a.uint_opt("depth"),
        )?),
        "graph_rename" => ok_text(crate::engine::graph_rename(
            path, a.str_req("name", "graph_rename")?, a.str_req("new_name", "graph_rename")?,
        )?),
        "graph_create" => ok_text(crate::engine::graph_create(
            path,
            a.str_req("file", "graph_create")?,
            a.str_req("function_name", "graph_create")?,
            a.str_opt("language"),
        )?),
        "graph_add" => ok_text(crate::engine::graph_add(
            path,
            a.str_req("entity_type", "graph_add")?,
            a.str_req("name", "graph_add")?,
            a.str_req("file", "graph_add")?,
            a.str_opt("after_symbol"),
            a.str_opt("language"),
        )?),
        "graph_move" => ok_text(crate::engine::graph_move(
            path, a.str_req("name", "graph_move")?, a.str_req("to_file", "graph_move")?,
        )?),
        "trace_down" => ok_text(crate::engine::trace_down(
            path, a.str_req("name", "trace_down")?, a.uint_opt("depth"), a.str_opt("file"),
        )?),
        "semantic_search" => ok_text(crate::engine::semantic_search(
            path, a.str_req("query", "semantic_search")?, a.uint_opt("limit"), a.str_opt("file"),
        )?),
        "health" => ok_text(crate::engine::health(path, a.uint_opt("top"))?),
        "graph_delete" => ok_text(crate::engine::graph_delete(
            path, a.str_req("name", "graph_delete")?, a.str_opt("file"),
            a.bool_opt("force").unwrap_or(false),
        )?),
        other => Err(anyhow::anyhow!("Unknown tool: {other}")),
    }
}
