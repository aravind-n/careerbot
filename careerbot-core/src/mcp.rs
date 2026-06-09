//! MCP (Model Context Protocol) server. Speaks JSON-RPC 2.0 over
//! stdin/stdout, exposing careerbot's tool surface to whatever MCP
//! client spawned us (typically Claude Code via `--mcp-config`).
//!
//! The transport is line-delimited JSON: one request per line in,
//! one response per line out. `notifications/initialized` and any
//! other notification (id-less) is consumed without a reply.
//!
//! Logging is intentionally on stderr (see [`crate::log`]) so it
//! never collides with the protocol stream on stdout.

use crate::agent::ToolKit;
use crate::agent::tool_dispatch::{all_tools, dispatch_tool, to_mcp_tools};
use crate::runtime::Runtime;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::warn;

const FALLBACK_PROTOCOL_VERSION: &str = "2024-11-05";

/// JSON-RPC error codes we emit. Hex values for the standard codes.
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INTERNAL_ERROR: i64 = -32603;

#[derive(Debug, Deserialize)]
struct Request {
    #[allow(dead_code)]
    jsonrpc: String,
    method: String,
    #[serde(default)]
    params: Value,
    /// Notifications have no `id` and get no response.
    #[serde(default)]
    id: Option<Value>,
}

#[derive(Debug, Serialize)]
struct Response {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
    id: Value,
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i64,
    message: String,
}

/// Run the MCP server until stdin EOF.
pub async fn run(runtime: Arc<Runtime>) -> std::io::Result<()> {
    let toolkit = ToolKit::in_process(runtime.tools.clone());
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = tokio::io::stdout();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(response) = handle_line(&toolkit, line).await {
            let mut bytes = serde_json::to_vec(&response).map_err(std::io::Error::other)?;
            bytes.push(b'\n');
            stdout.write_all(&bytes).await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}

/// Public for tests; the production loop in `run` calls this for each line.
pub async fn handle_line(toolkit: &ToolKit, line: &str) -> Option<Response> {
    let request: Request = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            return Some(error_response(
                Value::Null,
                PARSE_ERROR,
                format!("parse error: {e}"),
            ));
        }
    };

    let id = request.id.clone();
    let is_notification = id.is_none();

    let outcome: Result<Value, RpcError> = match request.method.as_str() {
        "initialize" => Ok(initialize_result(&request.params)),
        "tools/list" => Ok(tools_list_result()),
        "tools/call" => tools_call_result(toolkit, &request.params).await,
        // Documented MCP notifications we accept silently.
        "notifications/initialized" | "notifications/cancelled" => {
            if is_notification {
                return None;
            }
            Err(RpcError {
                code: INVALID_REQUEST,
                message: "notification method requires no id".into(),
            })
        }
        other => Err(RpcError {
            code: METHOD_NOT_FOUND,
            message: format!("unknown method: {other}"),
        }),
    };

    if is_notification {
        // Any reply attempt would be a protocol violation. The caller
        // sent something without an id, treat it as fire-and-forget.
        if let Err(err) = outcome {
            warn!(?err, method = %request.method, "notification produced error");
        }
        return None;
    }

    let id = id.unwrap_or(Value::Null);
    Some(match outcome {
        Ok(result) => Response {
            jsonrpc: "2.0",
            result: Some(result),
            error: None,
            id,
        },
        Err(error) => Response {
            jsonrpc: "2.0",
            result: None,
            error: Some(error),
            id,
        },
    })
}

fn initialize_result(params: &Value) -> Value {
    // Echo the caller's protocol version when present; fall back to the
    // version we know we implement.
    let version = params
        .get("protocolVersion")
        .and_then(|v| v.as_str())
        .unwrap_or(FALLBACK_PROTOCOL_VERSION)
        .to_string();
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {}},
        "serverInfo": {
            "name": "careerbot",
            "version": env!("CARGO_PKG_VERSION"),
        }
    })
}

fn tools_list_result() -> Value {
    json!({
        "tools": to_mcp_tools(&all_tools()),
    })
}

async fn tools_call_result(toolkit: &ToolKit, params: &Value) -> Result<Value, RpcError> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RpcError {
            code: INVALID_REQUEST,
            message: "missing or non-string 'name'".into(),
        })?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));

    let (text, is_error) = match dispatch_tool(toolkit, name, &arguments).await {
        Ok(s) => (s, false),
        Err(e) => (e, true),
    };

    Ok(json!({
        "content": [{"type": "text", "text": text}],
        "isError": is_error,
    }))
}

fn error_response(id: Value, code: i64, message: impl Into<String>) -> Response {
    Response {
        jsonrpc: "2.0",
        result: None,
        error: Some(RpcError {
            code,
            message: message.into(),
        }),
        id,
    }
}

#[cfg(test)]
impl Response {
    pub fn result_value(&self) -> Option<&Value> {
        self.result.as_ref()
    }
    pub fn error_code(&self) -> Option<i64> {
        self.error.as_ref().map(|e| e.code)
    }
    pub fn id_value(&self) -> &Value {
        &self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;
    use crate::tools::CoreTools;
    use std::sync::Arc;
    use tempfile::TempDir;

    async fn toolkit() -> (TempDir, ToolKit) {
        let dir = TempDir::new().unwrap();
        let paths = Paths::rooted_at(dir.path().join("data"), dir.path().join("state"));
        let pool = Arc::new(crate::db::open_memory().await.unwrap());
        let tools = CoreTools::with_script_runner(pool, paths, vec!["python3".into()]);
        (dir, ToolKit::in_process(Arc::new(tools)))
    }

    fn line(method: &str, params: Value, id: i64) -> String {
        format!(
            "{}",
            json!({"jsonrpc": "2.0", "method": method, "params": params, "id": id})
        )
    }

    #[tokio::test]
    async fn initialize_returns_protocol_version_and_server_info() {
        let (_dir, kit) = toolkit().await;
        let req = line("initialize", json!({"protocolVersion": "2024-11-05"}), 1);
        let resp = handle_line(&kit, &req).await.unwrap();
        assert_eq!(resp.id_value(), &json!(1));
        let result = resp.result_value().unwrap();
        assert_eq!(result["protocolVersion"], "2024-11-05");
        assert_eq!(result["serverInfo"]["name"], "careerbot");
        assert!(result["capabilities"]["tools"].is_object());
    }

    #[tokio::test]
    async fn initialize_falls_back_when_version_missing() {
        let (_dir, kit) = toolkit().await;
        let req = line("initialize", json!({}), 2);
        let resp = handle_line(&kit, &req).await.unwrap();
        let result = resp.result_value().unwrap();
        assert_eq!(result["protocolVersion"], FALLBACK_PROTOCOL_VERSION);
    }

    #[tokio::test]
    async fn tools_list_returns_the_eight_canonical_tools() {
        let (_dir, kit) = toolkit().await;
        let req = line("tools/list", json!({}), 3);
        let resp = handle_line(&kit, &req).await.unwrap();
        let tools = resp.result_value().unwrap()["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 8);
        assert_eq!(tools[0]["name"], "fetch_url");
        // MCP wire format uses camelCase.
        assert!(tools[0]["inputSchema"].is_object());
    }

    #[tokio::test]
    async fn tools_call_dispatches_to_core_tools() {
        let (_dir, kit) = toolkit().await;
        // Write the profile through the tool, then read it back.
        let write_req = line(
            "tools/call",
            json!({"name": "write_profile", "arguments": {"content": "# Profile\nhi"}}),
            4,
        );
        let write_resp = handle_line(&kit, &write_req).await.unwrap();
        assert_eq!(
            write_resp.result_value().unwrap()["isError"],
            false,
            "{:?}",
            write_resp.result_value()
        );

        let read_req = line(
            "tools/call",
            json!({"name": "read_profile", "arguments": {}}),
            5,
        );
        let read_resp = handle_line(&kit, &read_req).await.unwrap();
        let content = &read_resp.result_value().unwrap()["content"];
        assert_eq!(content[0]["text"], "# Profile\nhi");
    }

    #[tokio::test]
    async fn tools_call_with_unknown_tool_returns_is_error() {
        let (_dir, kit) = toolkit().await;
        let req = line(
            "tools/call",
            json!({"name": "does_not_exist", "arguments": {}}),
            6,
        );
        let resp = handle_line(&kit, &req).await.unwrap();
        let result = resp.result_value().unwrap();
        assert_eq!(result["isError"], true);
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("unknown tool"));
    }

    #[tokio::test]
    async fn unknown_method_returns_method_not_found() {
        let (_dir, kit) = toolkit().await;
        let req = line("does/not/exist", json!({}), 7);
        let resp = handle_line(&kit, &req).await.unwrap();
        assert_eq!(resp.error_code(), Some(METHOD_NOT_FOUND));
    }

    #[tokio::test]
    async fn parse_error_returns_response_with_null_id() {
        let (_dir, kit) = toolkit().await;
        let resp = handle_line(&kit, "not valid json").await.unwrap();
        assert_eq!(resp.error_code(), Some(PARSE_ERROR));
        assert_eq!(resp.id_value(), &Value::Null);
    }

    #[tokio::test]
    async fn notification_initialized_returns_no_response() {
        let (_dir, kit) = toolkit().await;
        let req = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        assert!(handle_line(&kit, req).await.is_none());
    }
}
