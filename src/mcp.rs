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
                "capabilities": {"tools": {"listChanged": true}},
                "serverInfo": {"name": "yoyo", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "You have access to yoyo, a code intelligence MCP server. \
                    Always call `llm_instructions` first on any new project to learn available tools and workflows. \
                    Call `bake` to build or refresh the index before using index-dependent tools. \
                    Use `supersearch` for all code search (replaces grep). \
                    Use `symbol` with include_source=true to read function bodies. \
                    Use `slice` to read arbitrary line ranges. \
                    Use `patch` or `patch_by_symbol` to write changes back."
            });

            JsonRpcResponse { jsonrpc: "2.0", id: req.id, result: Some(result), error: None }
        }
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
        tool("llm_instructions", "Prime directive and usage instructions for yoyo.", json!({"path": p()})),
        tool("shake", "Generate a high-level repository overview (languages, size, basic stats).", json!({"path": p()})),
        tool("bake", "Build and persist a bake index under the project root.", json!({"path": p()})),
        tool("symbol", "Detailed lookup of a function symbol from the bake index. When include_source is true, each match includes the function body inline. Use file to scope to one module and limit to cap result size.", json!({
            "path": p(),
            "name": s("Symbol (function) name to look up"),
            "include_source": b("If true, include the function body (source code) in each match"),
            "file": s("Optional file path substring to narrow results (e.g. 'routes/user' or 'tcp_core')"),
            "limit": i("Max matches to return (default 20). Lower when include_source=true to stay within context limits.")
        })),
        tool("all_endpoints", "List all detected API endpoints from the bake index.", json!({"path": p()})),
        tool("slice", "Read a specific line range of a file.", json!({
            "path": p(),
            "file": s("File path relative to the project root"),
            "start": i("1-based start line (inclusive)"),
            "end": i("1-based end line (inclusive)")
        })),
        tool("api_surface", "Exported API summary grouped by module (TypeScript-only for now).", json!({
            "path": p(),
            "package": s("Optional package/module filter (substring match on module or file paths)"),
            "limit": i("Maximum number of functions per module (default 20)")
        })),
        tool("file_functions", "Per-file function overview from the bake index.", json!({
            "path": p(),
            "file": s("File path relative to the project root"),
            "include_summaries": b("Whether to include summaries (currently a no-op placeholder)")
        })),
        tool("supersearch", "AST-aware search over TypeScript, Rust, Python, and Go source files. Prefer over grep. Use file to restrict scope and limit to cap noisy results.", json!({
            "path": p(),
            "query": s("Search query text"),
            "context": s("Search context: all | strings | comments | identifiers"),
            "pattern": s("Pattern: all | call | assign | return"),
            "exclude_tests": b("Whether to exclude likely test files"),
            "file": s("Optional file path substring to restrict scope (e.g. 'src/routes' or 'tcp')"),
            "limit": i("Max matches to return (default 200). Reduce for large codebases with common terms.")
        })),
        tool("package_summary", "Deep-dive summary of a package/module directory.", json!({
            "path": p(),
            "package": s("Package/module name or directory substring")
        })),
        tool("architecture_map", "Project structure map and placement hints.", json!({
            "path": p(),
            "intent": s("Intent description, e.g. \"user handler\" or \"auth service\"")
        })),
        tool("suggest_placement", "Suggest where to place a new function.", json!({
            "path": p(),
            "function_name": s("Name of the function to add"),
            "function_type": s("Function type: handler | service | repository | model | util | test"),
            "related_to": s("Existing related symbol or substring (optional)")
        })),
        tool("crud_operations", "Entity-level CRUD matrix inferred from endpoints.", json!({
            "path": p(),
            "entity": s("Optional entity filter (e.g. \"user\")")
        })),
        tool("api_trace", "Trace an API endpoint through backend handlers.", json!({
            "path": p(),
            "endpoint": s("Endpoint path (or substring), e.g. \"/users\""),
            "method": s("Optional HTTP method (GET, POST, etc.)")
        })),
        tool("find_docs", "Find documentation/config files.", json!({
            "path": p(),
            "doc_type": s("Documentation type: readme | env | config | docker | all")
        })),
        tool("patch", "Apply a patch to a file. Three modes: (1) by symbol name — pass 'name'; (2) by line range — pass 'file'+'start'+'end'; (3) by content match — pass 'file'+'old_string'+'new_string'. Mode 3 is immune to line drift and preferred for large edits.", json!({
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
        tool_req("patch_bytes", "Splice at exact byte offsets in a file. Use byte_start/byte_end from the bake index (IndexedFunction.byte_start / byte_end) for precise single-node replacement.", &["file", "byte_start", "byte_end", "new_content"], json!({
            "path": p(),
            "file": s("File path relative to project root"),
            "byte_start": i("Inclusive start byte offset"),
            "byte_end": i("Exclusive end byte offset"),
            "new_content": s("Replacement text")
        })),
        tool_req("multi_patch", "Apply N byte-level edits across M files atomically. Edits are applied bottom-up per file so offsets remain valid. Each file is written exactly once. Use for graph-level mutations such as renaming a symbol across all call sites.", &["edits"], json!({
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
        tool_req("blast_radius", "Analyse the blast radius of a symbol: find all functions that (transitively) call it, and the set of affected files. Requires a prior bake.", &["symbol"], json!({
            "path": p(),
            "symbol": s("Function name to analyse (exact match on the callee name)"),
            "depth": i("Maximum call-graph depth to traverse (default 2)")
        })),
        tool_req("graph_rename", "Rename a symbol everywhere — definition + all call sites — atomically. Uses word-boundary matching so partial identifier names are not affected. Reindexes affected files automatically.", &["name", "new_name"], json!({
            "path": p(),
            "name": s("Current identifier name to rename"),
            "new_name": s("New identifier name")
        })),
        tool_req("graph_add", "Insert a new function or struct scaffold into a file. Optionally place it after an existing symbol (resolved from the bake index). Reindexes the file automatically.", &["entity_type", "name", "file"], json!({
            "path": p(),
            "entity_type": s("Scaffold type: fn (Rust) | function (TS/JS) | def (Python) | func (Go)"),
            "name": s("Name for the new function/entity"),
            "file": s("File path relative to project root"),
            "after_symbol": s("Optional: insert after this existing symbol (name or substring)"),
            "language": s("Optional: override language detection (rust | typescript | python | go)")
        })),
        tool_req("graph_move", "Move a function from its current file to another file. Removes from source, appends to destination, reindexes both. Requires a prior bake.", &["name", "to_file"], json!({
            "path": p(),
            "name": s("Exact function name to move (matched case-insensitively in bake index)"),
            "to_file": s("Destination file path relative to project root")
        })),
        tool_req("trace_down", "Trace a function's call chain downward to its leaves and external boundaries (database, http_client, queue). Scoped to Go and Rust. Requires a prior bake.", &["name"], json!({
            "path": p(),
            "name": s("Function name to start the trace from"),
            "depth": i("Maximum call depth to follow (default 5)"),
            "file": s("Optional file path substring to disambiguate when multiple functions share the same name")
        })),
        tool("health", "Diagnose codebase health: dead code (never-called functions), god functions (high complexity × fan-out), and duplicate hints (same-stem functions across different files). Run after bake.", json!({
            "path": p(),
            "top": i("Max results per category (default 10)")
        })),
        tool_req("graph_delete", "Remove a function from a file by name. Erases its byte range and reindexes. Use health or blast_radius to confirm it is safe to delete.", &["name"], json!({
            "path": p(),
            "name": s("Exact function name to delete (matched case-insensitively in bake index)"),
            "file": s("Optional file path substring to disambiguate when multiple functions share the same name")
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
            a.uint_req("start", "slice")? as u32,
            a.uint_req("end", "slice")? as u32,
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
        "health" => ok_text(crate::engine::health(path, a.uint_opt("top"))?),
        "graph_delete" => ok_text(crate::engine::graph_delete(
            path, a.str_req("name", "graph_delete")?, a.str_opt("file"),
        )?),
        other => Err(anyhow::anyhow!("Unknown tool: {other}")),
    }
}
