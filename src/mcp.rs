use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

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

/// Run a very small MCP-compatible JSON-RPC server over stdin/stdout.
///
/// For now this only supports list_tools and a stub llm_instructions tool.
pub async fn run_stdio_server() -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::new(stdin).lines();
    let mut writer = stdout;

    while let Some(line) = reader.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(err) => {
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id: None,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {err}"),
                    }),
                };
                let json = serde_json::to_string(&resp)?;
                writer.write_all(json.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
                continue;
            }
        };

        let resp = handle_request(req).await;
        let json = serde_json::to_string(&resp)?;
        writer.write_all(json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
    }

    Ok(())
}

async fn handle_request(req: JsonRpcRequest) -> JsonRpcResponse {
    match req.method.as_str() {
        "list_tools" => {
            let tools = list_tools();
            JsonRpcResponse {
                jsonrpc: "2.0",
                id: req.id,
                result: Some(tools),
                error: None,
            }
        }
        "call_tool" => {
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
                "description": "Detailed lookup of a function symbol from the bake index.",
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
                "description": "Text-based search over TS/JS source files.",
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
                            "description": "Search context: all | strings | comments | identifiers (currently best-effort)"
                        },
                        "pattern": {
                            "type": "string",
                            "description": "Pattern: all | call | assign | return (currently best-effort)"
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
                ]
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
                ]
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
                ]
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
                ]
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
            let json = crate::engine::symbol(path, name)?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": json
                    }
                ]
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
                ]
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
                ]
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
                ]
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
                ]
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
                ]
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
                ]
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
                ]
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
                ]
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
                ]
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
                ]
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
                ]
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
                ]
            }))
        }
        other => Err(anyhow::anyhow!("Unknown tool: {other}")),
    }
}

