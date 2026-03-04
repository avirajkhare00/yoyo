use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// Minimal JSON-RPC 2.0 request.
#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// Minimal JSON-RPC 2.0 response.
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
            // EOF
            return Ok(());
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            // Skip empty lines.
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            // Content-Length framed request.
            let mut content_length: Option<usize> = None;
            if let Ok(len) = rest.trim().parse::<usize>() {
                content_length = Some(len);
            }

            // Consume remaining headers until blank line.
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
            // Line-delimited JSON-RPC (no framing). This is what Claude Desktop
            // currently sends/accepts over stdio.
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
            // Unknown prefix; ignore and continue.
            continue;
        }
    }
}

async fn handle_request(req: JsonRpcRequest) -> JsonRpcResponse {
    match req.method.as_str() {
        "initialize" => {
            // Minimal MCP initialize handshake.
            let protocol_version = req
                .params
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("2025-11-25");

            let result = serde_json::json!({
                "protocolVersion": protocol_version,
                "capabilities": {
                    "tools": {
                        "listChanged": true
                    }
                },
                "serverInfo": {
                    "name": "yoyo",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            });

            JsonRpcResponse {
                jsonrpc: "2.0",
                id: req.id,
                result: Some(result),
                error: None,
            }
        }
        "list_tools" | "tools/list" => {
            let tools = list_tools();
            JsonRpcResponse {
                jsonrpc: "2.0",
                id: req.id,
                result: Some(tools),
                error: None,
            }
        }
        "call_tool" | "tools/call" => {
            let result = call_tool(req.params).await;
            match result {
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
            }
        }
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

fn list_tools() -> Value {
    // Minimal MCP tools list with core CLI-equivalent tools for yoyo.
    // Include an explicit null nextCursor to match MCP tools/list shape.
    serde_json::json!({
        "tools": [
            {
                "name": "llm_instructions",
                "description": "Prime directive and usage instructions for yoyo.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional path to project directory"
                        }
                    }
                }
            },
            {
                "name": "shake",
                "description": "Generate a high-level repository overview (languages, size, basic stats).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional path to project directory"
                        }
                    }
                }
            },
            {
                "name": "bake",
                "description": "Build and persist a bake index under the project root.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional path to project directory"
                        }
                    }
                }
            },
            {
                "name": "search",
                "description": "Fuzzy search over functions and files from the bake index.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional path to project directory"
                        },
                        "q": {
                            "type": "string",
                            "description": "Search query text"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results for functions and files (default 10)"
                        }
                    }
                }
            },
            {
                "name": "symbol",
                "description": "Detailed lookup of a function symbol from the bake index. When include_source is true, each match includes the function body inline (no need to call slice separately).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional path to project directory"
                        },
                        "name": {
                            "type": "string",
                            "description": "Symbol (function) name to look up"
                        },
                        "include_source": {
                            "type": "boolean",
                            "description": "If true, include the function body (source code) in each match"
                        }
                    }
                }
            },
            {
                "name": "all_endpoints",
                "description": "List all detected API endpoints from the bake index.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional path to project directory"
                        }
                    }
                }
            },
            {
                "name": "slice",
                "description": "Read a specific line range of a file.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional path to project directory"
                        },
                        "file": {
                            "type": "string",
                            "description": "File path relative to the project root"
                        },
                        "start": {
                            "type": "integer",
                            "description": "1-based start line (inclusive)"
                        },
                        "end": {
                            "type": "integer",
                            "description": "1-based end line (inclusive)"
                        }
                    }
                }
            },
            {
                "name": "api_surface",
                "description": "Exported API summary grouped by module (TypeScript-only for now).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional path to project directory"
                        },
                        "package": {
                            "type": "string",
                            "description": "Optional package/module filter (substring match on module or file paths)"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of functions per module (default 20)"
                        }
                    }
                }
            },
            {
                "name": "file_functions",
                "description": "Per-file function overview from the bake index.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional path to project directory"
                        },
                        "file": {
                            "type": "string",
                            "description": "File path relative to the project root"
                        },
                        "include_summaries": {
                            "type": "boolean",
                            "description": "Whether to include summaries (currently a no-op placeholder)"
                        }
                    }
                }
            },
            {
                "name": "supersearch",
                "description": "AST-aware search over TypeScript, Rust, and Python source files.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional path to project directory"
                        },
                        "query": {
                            "type": "string",
                            "description": "Search query text"
                        },
                        "context": {
                            "type": "string",
                            "description": "Search context: all | strings | comments | identifiers"
                        },
                        "pattern": {
                            "type": "string",
                            "description": "Pattern: all | call | assign | return"
                        },
                        "exclude_tests": {
                            "type": "boolean",
                            "description": "Whether to exclude likely test files"
                        }
                    }
                }
            },
            {
                "name": "package_summary",
                "description": "Deep-dive summary of a package/module directory.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional path to project directory"
                        },
                        "package": {
                            "type": "string",
                            "description": "Package/module name or directory substring"
                        }
                    }
                }
            },
            {
                "name": "architecture_map",
                "description": "Project structure map and placement hints.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional path to project directory"
                        },
                        "intent": {
                            "type": "string",
                            "description": "Intent description, e.g. \"user handler\" or \"auth service\""
                        }
                    }
                }
            },
            {
                "name": "suggest_placement",
                "description": "Suggest where to place a new function.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional path to project directory"
                        },
                        "function_name": {
                            "type": "string",
                            "description": "Name of the function to add"
                        },
                        "function_type": {
                            "type": "string",
                            "description": "Function type: handler | service | repository | model | util | test"
                        },
                        "related_to": {
                            "type": "string",
                            "description": "Existing related symbol or substring (optional)"
                        }
                    }
                }
            },
            {
                "name": "crud_operations",
                "description": "Entity-level CRUD matrix inferred from endpoints.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional path to project directory"
                        },
                        "entity": {
                            "type": "string",
                            "description": "Optional entity filter (e.g. \"user\")"
                        }
                    }
                }
            },
            {
                "name": "api_trace",
                "description": "Trace an API endpoint through backend handlers.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional path to project directory"
                        },
                        "endpoint": {
                            "type": "string",
                            "description": "Endpoint path (or substring), e.g. \"/users\""
                        },
                        "method": {
                            "type": "string",
                            "description": "Optional HTTP method (GET, POST, etc.)"
                        }
                    }
                }
            },
            {
                "name": "find_docs",
                "description": "Find documentation/config files.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional path to project directory"
                        },
                        "doc_type": {
                            "type": "string",
                            "description": "Documentation type: readme | env | config | docker | all"
                        }
                    }
                }
            },
            {
                "name": "patch",
                "description": "Apply a line-range patch to a file.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional path to project directory"
                        },
                        "file": {
                            "type": "string",
                            "description": "File path relative to the project root"
                        },
                        "start": {
                            "type": "integer",
                            "description": "1-based start line (inclusive)"
                        },
                        "end": {
                            "type": "integer",
                            "description": "1-based end line (inclusive)"
                        },
                        "new_content": {
                            "type": "string",
                            "description": "Replacement content for the specified line range"
                        }
                    }
                }
            }
        ]
    })
}

#[derive(Debug, Deserialize)]
struct CallToolParams {
    pub name: String,
    #[allow(dead_code)]
    #[serde(default)]
    pub arguments: Value,
}

async fn call_tool(params: Value) -> Result<Value> {
    let p: CallToolParams = serde_json::from_value(params)?;

    match p.name.as_str() {
        "llm_instructions" => {
            let path = p
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let json = crate::engine::llm_instructions(path)?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": json
                    }
                ],
                "isError": false
            }))
        }
        "shake" => {
            let path = p
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let json = crate::engine::shake(path)?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": json
                    }
                ],
                "isError": false
            }))
        }
        "bake" => {
            let path = p
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let json = crate::engine::bake(path)?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": json
                    }
                ],
                "isError": false
            }))
        }
        "search" => {
            let path = p
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let q = p
                .arguments
                .get("q")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("Missing required 'q' argument for search"))?;
            let limit = p
                .arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize);
            let json = crate::engine::search(path, q, limit)?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": json
                    }
                ],
                "isError": false
            }))
        }
        "symbol" => {
            let path = p
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let name = p
                .arguments
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("Missing required 'name' argument for symbol"))?;
            let include_source = p
                .arguments
                .get("include_source")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let json = crate::engine::symbol(path, name, include_source)?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": json
                    }
                ],
                "isError": false
            }))
        }
        "all_endpoints" => {
            let path = p
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let json = crate::engine::all_endpoints(path)?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": json
                    }
                ],
                "isError": false
            }))
        }
        "slice" => {
            let path = p
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let file = p
                .arguments
                .get("file")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("Missing required 'file' argument for slice"))?;
            let start = p
                .arguments
                .get("start")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Missing required 'start' argument for slice"))?
                as u32;
            let end = p
                .arguments
                .get("end")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Missing required 'end' argument for slice"))?
                as u32;
            let json = crate::engine::slice(path, file, start, end)?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": json
                    }
                ],
                "isError": false
            }))
        }
        "api_surface" => {
            let path = p
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let package = p
                .arguments
                .get("package")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let limit = p
                .arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize);
            let json = crate::engine::api_surface(path, package, limit)?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": json
                    }
                ],
                "isError": false
            }))
        }
        "file_functions" => {
            let path = p
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let file = p
                .arguments
                .get("file")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("Missing required 'file' argument for file_functions"))?;
            let include_summaries = p
                .arguments
                .get("include_summaries")
                .and_then(|v| v.as_bool());
            let json = crate::engine::file_functions(path, file, include_summaries)?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": json
                    }
                ],
                "isError": false
            }))
        }
        "supersearch" => {
            let path = p
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let query = p
                .arguments
                .get("query")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("Missing required 'query' argument for supersearch"))?;
            let context = p
                .arguments
                .get("context")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "all".to_string());
            let pattern = p
                .arguments
                .get("pattern")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "all".to_string());
            let exclude_tests = p
                .arguments
                .get("exclude_tests")
                .and_then(|v| v.as_bool());
            let json =
                crate::engine::supersearch(path, query, context, pattern, exclude_tests)?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": json
                    }
                ],
                "isError": false
            }))
        }
        "package_summary" => {
            let path = p
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let package = p
                .arguments
                .get("package")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("Missing required 'package' argument for package_summary"))?;
            let json = crate::engine::package_summary(path, package)?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": json
                    }
                ],
                "isError": false
            }))
        }
        "architecture_map" => {
            let path = p
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let intent = p
                .arguments
                .get("intent")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("Missing required 'intent' argument for architecture_map"))?;
            let json = crate::engine::architecture_map(path, intent)?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": json
                    }
                ],
                "isError": false
            }))
        }
        "suggest_placement" => {
            let path = p
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let function_name = p
                .arguments
                .get("function_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("Missing required 'function_name' argument for suggest_placement"))?;
            let function_type = p
                .arguments
                .get("function_type")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("Missing required 'function_type' argument for suggest_placement"))?;
            let related_to = p
                .arguments
                .get("related_to")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let json = crate::engine::suggest_placement(path, function_name, function_type, related_to)?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": json
                    }
                ],
                "isError": false
            }))
        }
        "crud_operations" => {
            let path = p
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let entity = p
                .arguments
                .get("entity")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let json = crate::engine::crud_operations(path, entity)?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": json
                    }
                ],
                "isError": false
            }))
        }
        "api_trace" => {
            let path = p
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let endpoint = p
                .arguments
                .get("endpoint")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("Missing required 'endpoint' argument for api_trace"))?;
            let method = p
                .arguments
                .get("method")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let json = crate::engine::api_trace(path, endpoint, method)?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": json
                    }
                ],
                "isError": false
            }))
        }
        "find_docs" => {
            let path = p
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let doc_type = p
                .arguments
                .get("doc_type")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("Missing required 'doc_type' argument for find_docs"))?;
            let json = crate::engine::find_docs(path, doc_type)?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": json
                    }
                ],
                "isError": false
            }))
        }
        "patch" => {
            let path = p
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let file = p
                .arguments
                .get("file")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("Missing required 'file' argument for patch"))?;
            let start = p
                .arguments
                .get("start")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Missing required 'start' argument for patch"))?
                as u32;
            let end = p
                .arguments
                .get("end")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Missing required 'end' argument for patch"))?
                as u32;
            let new_content = p
                .arguments
                .get("new_content")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("Missing required 'new_content' argument for patch"))?;
            let json = crate::engine::patch(path, file, start, end, new_content)?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": json
                    }
                ],
                "isError": false
            }))
        }
        other => Err(anyhow::anyhow!("Unknown tool: {other}")),
    }
}

