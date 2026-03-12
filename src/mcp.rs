use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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
                "You have access to yoyo, a code intelligence MCP server -- {n_tools} tools to read and edit any codebase from the AST. \
                    ON FIRST CONTACT: call `boot` and `index` in parallel. \
                    `boot` returns tool names grouped by category and concurrency rules. \
                    `index` builds the AST index all read-indexed tools depend on. \
                    Call `help(name)` to get params, output shape, and examples for any tool before using it. \
                    Key combos: health->callers->delete (safe dead code removal), flow->symbol->bulk_edit (fix endpoint end-to-end), callers->rename->symbol (safe rename). \
                    REPLACEMENTS: search replaces grep/rg. symbol+include_source replaces cat/Read. read replaces line-range reads. edit replaces Edit for function-level changes."
            );
            let result = json!({
                "protocolVersion": protocol_version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "yoyo", "version": env!("CARGO_PKG_VERSION")},
                "instructions": instructions
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

fn build_registry() -> Vec<ToolEntry> {
    fn s(desc: &str) -> Value { json!({"type": "string", "description": desc}) }
    fn i(desc: &str) -> Value { json!({"type": "integer", "description": desc}) }
    fn b(desc: &str) -> Value { json!({"type": "boolean", "description": desc}) }
    fn p() -> Value { s("Optional path to project directory") }

    let catalog: std::collections::HashMap<&'static str, &'static str> =
        crate::engine::tool_catalog().into_iter().map(|t| (t.name, t.description)).collect();
    let d = |name: &'static str| -> &'static str { catalog.get(name).copied().unwrap_or(name) };

    fn schema(name: &str, desc: &str, props: Value) -> Value {
        json!({"name": name, "description": desc, "inputSchema": {"type": "object", "properties": props}})
    }
    fn schema_req(name: &str, desc: &str, req: &[&str], props: Value) -> Value {
        json!({"name": name, "description": desc, "inputSchema": {"type": "object", "required": req, "properties": props}})
    }

    vec![
        // ── bootstrap ────────────────────────────────────────────────────────
        ToolEntry {
            schema: schema("boot", d("boot"), json!({"path": p()})),
            handler: Box::new(|_a, path| crate::engine::llm_instructions(path)),
        },
        ToolEntry {
            schema: schema("index", d("index"), json!({"path": p()})),
            handler: Box::new(|_a, path| crate::engine::bake(path)),
        },
        // ── read ─────────────────────────────────────────────────────────────
        ToolEntry {
            schema: schema("read", d("read"), json!({
                "path": p(),
                "file": s("File path relative to the project root"),
                "start_line": i("1-based start line (inclusive)."),
                "end_line": i("1-based end line (inclusive).")
            })),
            handler: Box::new(|a, path| crate::engine::slice(
                path,
                a.str_req("file", "read")?,
                a.uint_req("start_line", "read")? as u32,
                a.uint_req("end_line", "read")? as u32,
            )),
        },
        // ── read-indexed ─────────────────────────────────────────────────────
        ToolEntry {
            schema: schema("map", d("map"), json!({
                "path": p(),
                "intent": s("Intent description, e.g. \"user handler\" or \"auth service\""),
                "limit": {"type": "integer", "description": "Max directories to return (default 100)."}
            })),
            handler: Box::new(|a, path| crate::engine::architecture_map(path, a.str_opt("intent"), a.uint_opt("limit"))),
        },
        ToolEntry {
            schema: schema("symbol", d("symbol"), json!({
                "path": p(),
                "name": s("Symbol (function) name to look up"),
                "include_source": b("If true, include the function body in each match"),
                "file": s("Optional file path substring to narrow results"),
                "limit": i("Max matches to return (default 20)."),
                "stdlib": b("If true, also search installed toolchain stdlibs")
            })),
            handler: Box::new(|a, path| crate::engine::symbol(
                path,
                a.str_req("name", "symbol")?,
                a.bool_opt("include_source").unwrap_or(false),
                a.str_opt("file"),
                a.uint_opt("limit"),
                a.bool_opt("stdlib").unwrap_or(false),
            )),
        },
        ToolEntry {
            schema: schema("outline", d("outline"), json!({
                "path": p(),
                "file": s("File path relative to the project root"),
                "include_summaries": b("Whether to include summaries (currently a no-op placeholder)")
            })),
            handler: Box::new(|a, path| crate::engine::file_functions(
                path,
                a.str_req("file", "outline")?,
                a.bool_opt("include_summaries"),
            )),
        },
        ToolEntry {
            schema: schema("search", d("search"), json!({
                "path": p(),
                "query": s("Search query text"),
                "context": s("Search context: all | strings | comments | identifiers"),
                "pattern": s("Pattern: all | call | assign | return"),
                "exclude_tests": b("Whether to exclude likely test files"),
                "file": s("Optional file path substring to restrict scope"),
                "limit": i("Max matches to return (default 200).")
            })),
            handler: Box::new(|a, path| {
                const MODES: &[&str] = &["all", "call", "assign", "return"];
                let raw_pattern = a.str_opt("pattern");
                let query = if let Some(q) = a.str_opt("query") {
                    q
                } else if let Some(ref p) = raw_pattern {
                    if !MODES.contains(&p.as_str()) {
                        p.clone()
                    } else {
                        return Err(anyhow::anyhow!("Missing required 'query' argument for search"));
                    }
                } else {
                    return Err(anyhow::anyhow!("Missing required 'query' argument for search"));
                };
                let pattern = raw_pattern
                    .filter(|p| MODES.contains(&p.as_str()))
                    .unwrap_or_else(|| "all".to_string());
                crate::engine::supersearch(
                    path,
                    query,
                    a.str_opt("context").unwrap_or_else(|| "all".to_string()),
                    pattern,
                    a.bool_opt("exclude_tests"),
                    a.str_opt("file"),
                    a.uint_opt("limit"),
                )
            }),
        },
        ToolEntry {
            schema: schema("ask", d("ask"), json!({
                "path": p(),
                "query": s("Natural-language description, e.g. 'validate user token'"),
                "limit": i("Max results (default 10, max 50)"),
                "file": s("Optional file path substring to restrict scope")
            })),
            handler: Box::new(|a, path| crate::engine::semantic_search(
                path,
                a.str_req("query", "ask")?,
                a.uint_opt("limit"),
                a.str_opt("file"),
            )),
        },
        ToolEntry {
            schema: schema("routes", d("routes"), json!({"path": p()})),
            handler: Box::new(|_a, path| crate::engine::all_endpoints(path)),
        },
        ToolEntry {
            schema: schema_req("flow", d("flow"), &["endpoint"], json!({
                "path": p(),
                "endpoint": s("URL path substring to match (e.g. '/users')"),
                "method": s("Optional HTTP method filter (GET, POST, PUT, DELETE, PATCH)"),
                "depth": i("Max call chain depth (default 5)"),
                "include_source": b("If true, include the handler function source inline")
            })),
            handler: Box::new(|a, path| crate::engine::flow(
                path,
                a.str_req("endpoint", "flow")?,
                a.str_opt("method"),
                a.uint_opt("depth"),
                a.bool_opt("include_source").unwrap_or(false),
            )),
        },
        ToolEntry {
            schema: schema_req("callers", d("callers"), &["symbol"], json!({
                "path": p(),
                "symbol": s("Function name to analyse"),
                "depth": i("Maximum call-graph depth to traverse (default 2)")
            })),
            handler: Box::new(|a, path| crate::engine::blast_radius(
                path,
                a.str_req("symbol", "callers")?,
                a.uint_opt("depth"),
            )),
        },
        ToolEntry {
            schema: schema("health", d("health"), json!({
                "path": p(),
                "top": i("Max results per category (default 10)"),
                "view": s("Response view: compact | full | raw"),
                "limit": i("Items per section when view=compact (default 3)"),
                "cursor": s("Section cursor in the form <section>:<offset>")
            })),
            handler: Box::new(|a, path| crate::engine::health(
                path,
                a.uint_opt("top"),
                a.str_opt("view"),
                a.uint_opt("limit"),
                a.str_opt("cursor"),
            )),
        },
        // ── write ────────────────────────────────────────────────────────────
        ToolEntry {
            schema: schema("edit", d("edit"), json!({
                "path": p(),
                "name": s("Symbol name to edit (resolves location from index)."),
                "match_index": i("0-based index when multiple symbols match name (default 0)"),
                "file": s("File path relative to project root (for range-based or content-match edit)"),
                "start": i("1-based start line (inclusive), for range-based edit"),
                "end": i("1-based end line (inclusive), for range-based edit"),
                "new_content": s("Replacement content for range-based edit"),
                "old_string": s("Exact string to find and replace (content-match mode)"),
                "new_string": s("Replacement string for content-match mode")
            })),
            handler: Box::new(|a, path| {
                if let Some(old_string) = a.str_opt("old_string") {
                    let new_string = a.str_req("new_string", "edit")?;
                    crate::engine::patch_string(path, a.str_req("file", "edit")?, old_string, new_string)
                } else {
                    let new_content = a.str_req("new_content", "edit")?;
                    if let Some(name) = a.str_opt("name") {
                        crate::engine::patch_by_symbol(path, name, new_content, a.uint_opt("match_index"))
                    } else {
                        crate::engine::patch(
                            path,
                            a.str_req("file", "edit")?,
                            a.uint_req("start", "edit")? as u32,
                            a.uint_req("end", "edit")? as u32,
                            new_content,
                        )
                    }
                }
            }),
        },
        ToolEntry {
            schema: schema_req("bulk_edit", d("bulk_edit"), &["edits"], json!({
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
            handler: Box::new(|a, path| {
                let edits_val = a.0.get("edits").and_then(|v| v.as_array())
                    .ok_or_else(|| anyhow::anyhow!("Missing required 'edits' argument for bulk_edit"))?;
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
                crate::engine::multi_patch(path, edits)
            }),
        },
        ToolEntry {
            schema: schema_req("rename", d("rename"), &["name", "new_name"], json!({
                "path": p(),
                "name": s("Current identifier name to rename"),
                "new_name": s("New identifier name")
            })),
            handler: Box::new(|a, path| crate::engine::graph_rename(
                path,
                a.str_req("name", "rename")?,
                a.str_req("new_name", "rename")?,
            )),
        },
        ToolEntry {
            schema: schema_req("create", d("create"), &["file", "function_name"], json!({
                "path": p(),
                "file": s("File path relative to project root"),
                "function_name": s("Name for the initial scaffolded function"),
                "language": s("Optional: override language detection"),
                "params": s("Optional: typed parameters as [{\"name\":\"x\",\"type\":\"i32\"},...]"),
                "returns": s("Optional: return type string")
            })),
            handler: Box::new(|a, path| {
                let params = parse_params(a.0.get("params"));
                crate::engine::graph_create(
                    path,
                    a.str_req("file", "create")?,
                    a.str_req("function_name", "create")?,
                    a.str_opt("language"),
                    params,
                    a.str_opt("returns"),
                )
            }),
        },
        ToolEntry {
            schema: schema_req("add", d("add"), &["entity_type", "name", "file"], json!({
                "path": p(),
                "entity_type": s("Scaffold type: fn (Rust) | function (TS/JS) | def (Python) | func (Go) | test"),
                "name": s("Name for the new function/entity"),
                "file": s("File path relative to project root"),
                "after_symbol": s("Optional: insert after this existing symbol"),
                "language": s("Optional: override language detection"),
                "params": s("Optional: typed parameters as [{\"name\":\"x\",\"type\":\"i32\"},...]"),
                "returns": s("Optional: return type string"),
                "on": s("Optional: struct/class name for impl/class block")
            })),
            handler: Box::new(|a, path| {
                let params = parse_params(a.0.get("params"));
                crate::engine::graph_add(
                    path,
                    a.str_req("entity_type", "add")?,
                    a.str_req("name", "add")?,
                    a.str_req("file", "add")?,
                    a.str_opt("after_symbol"),
                    a.str_opt("language"),
                    params,
                    a.str_opt("returns"),
                    a.str_opt("on"),
                )
            }),
        },
        ToolEntry {
            schema: schema_req("move", d("move"), &["name", "to_file"], json!({
                "path": p(),
                "name": s("Exact function name to move"),
                "to_file": s("Destination file path relative to project root")
            })),
            handler: Box::new(|a, path| crate::engine::graph_move(
                path,
                a.str_req("name", "move")?,
                a.str_req("to_file", "move")?,
            )),
        },
        ToolEntry {
            schema: schema_req("delete", d("delete"), &["name"], json!({
                "path": p(),
                "name": s("Exact function name to delete"),
                "file": s("Optional file path substring to disambiguate"),
                "force": b("Delete even if active callers exist (default false)")
            })),
            handler: Box::new(|a, path| crate::engine::graph_delete(
                path,
                a.str_req("name", "delete")?,
                a.str_opt("file"),
                a.bool_opt("force").unwrap_or(false),
            )),
        },
        // ── orchestration ────────────────────────────────────────────────────
        ToolEntry {
            schema: schema_req("script", d("script"), &["code"], json!({
                "path": p(),
                "code": s("Rhai script to execute. Read tools: symbol(name), blast_radius(name), health(), supersearch(query), file_functions(file), flow(endpoint, method), slice(file, start, end). Write tools: graph_delete(name).")
            })),
            handler: Box::new(|a, path| {
                crate::engine::run_script(path, a.str_req("code", "script")?)
            }),
        },
        // ── discovery ────────────────────────────────────────────────────────
        ToolEntry {
            schema: schema_req("help", d("help"), &["name"], json!({
                "name": s("Tool name to get help for")
            })),
            handler: Box::new(|a, _path| {
                crate::engine::tool_help(a.str_req("name", "help")?)
            }),
        },
    ]
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
        self.0.get(key).and_then(|v| {
            v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
        }).map(|n| n as usize)
    }
    fn uint_req(&self, key: &str, tool: &str) -> Result<u64> {
        self.0.get(key).and_then(|v| {
            v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
        }).ok_or_else(|| anyhow::anyhow!("Missing required '{}' argument for {}", key, tool))
    }
}

/// Parse `[{"name":"x","type":"i32"},...]` into `Vec<Param>`. Returns None if absent or malformed.
fn parse_params(val: Option<&Value>) -> Option<Vec<crate::engine::Param>> {
    let arr = val?.as_array()?;
    let params: Vec<_> = arr.iter().filter_map(|item| {
        let name = item.get("name")?.as_str()?.to_string();
        let type_str = item.get("type")?.as_str()?.to_string();
        Some(crate::engine::Param { name, type_str })
    }).collect();
    if params.is_empty() { None } else { Some(params) }
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
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/sample_project")
    }

    fn copy_dir(src: &Path, dst: &Path) {
        std::fs::create_dir_all(dst).unwrap();
        for entry in std::fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let s = entry.path();
            let d = dst.join(entry.file_name());
            if s.is_dir() { copy_dir(&s, &d); } else { std::fs::copy(&s, &d).unwrap(); }
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

    #[test]
    fn supersearch_pattern_alias_promotes_non_mode_value_to_query() {
        let dir = baked_fixture();
        let root = dir.path().to_string_lossy().into_owned();
        // Pass `pattern="add"` with no `query` — "add" is not a mode, should be promoted
        let result = rt().block_on(call_tool(json!({
            "name": "search",
            "arguments": {"path": root, "pattern": "add", "context": "identifiers"}
        })));
        assert!(result.is_ok(), "should succeed when pattern is not a mode value: {:?}", result.err());
        let text = result.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(
            v["matches"].as_array().map(|a| !a.is_empty()).unwrap_or(false),
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
        assert!(result.is_ok(), "should succeed with explicit query and pattern mode: {:?}", result.err());
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
        assert!(result.is_err(), "should error when pattern is a mode value and query is absent");
    }

    #[test]
    fn registry_and_catalog_names_are_in_sync() {
        let catalog_names: HashSet<&str> =
            crate::engine::tool_catalog().iter().map(|t| t.name).collect();
        let registry_names: HashSet<&str> =
            get_registry().iter().map(|t| t.name()).collect();

        let only_in_catalog: Vec<_> = catalog_names.difference(&registry_names).copied().collect();
        let only_in_registry: Vec<_> = registry_names.difference(&catalog_names).copied().collect();

        assert!(only_in_catalog.is_empty(),
            "In tool_catalog() but not build_registry(): {:?}", only_in_catalog);
        assert!(only_in_registry.is_empty(),
            "In build_registry() but not tool_catalog(): {:?}", only_in_registry);
    }
}
