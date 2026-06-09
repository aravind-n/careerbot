//! Shared tool definitions and dispatch logic for every in-process
//! caller — used by both [`AnthropicApiDriver`](super::anthropic_api)
//! (which forwards tool calls coming back over `/v1/messages`) and the
//! [`mcp`](crate::mcp) server (which forwards them coming in over
//! JSON-RPC stdio from a subprocess driver). One canonical list of
//! eight tools, two presentation formats.

use super::ToolKit;
use crate::types::Filters;
use serde_json::{Value, json};

/// One tool the agent can call. Schemas are plain `serde_json` values
/// because they're consumed by both Anthropic's `snake_case`
/// `input_schema` and MCP's camelCase `inputSchema`.
pub struct ToolSchema {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// Returns the canonical list of agent-facing tools. `record_run` and
/// `record_token_usage` are deliberately omitted — those are
/// bookkeeping the daemon does for the agent, not tools the agent
/// invokes.
pub fn all_tools() -> Vec<ToolSchema> {
    vec![
        ToolSchema {
            name: "fetch_url",
            description: "HTTP GET against a URL. Returns the response body as text.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string"},
                    "headers": {
                        "type": "object",
                        "additionalProperties": {"type": "string"}
                    }
                },
                "required": ["url"]
            }),
        },
        ToolSchema {
            name: "save_script",
            description: "Write a per-company Python collector script to the scripts directory.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "company": {"type": "string"},
                    "code": {"type": "string"}
                },
                "required": ["company", "code"]
            }),
        },
        ToolSchema {
            name: "run_script",
            description: "Execute the per-company script via `uv run`. Returns the parsed list of jobs as JSON.",
            input_schema: json!({
                "type": "object",
                "properties": {"company": {"type": "string"}},
                "required": ["company"]
            }),
        },
        ToolSchema {
            name: "read_profile",
            description: "Read the current profile.md.",
            input_schema: json!({"type": "object", "properties": {}}),
        },
        ToolSchema {
            name: "write_profile",
            description: "Overwrite profile.md with the given markdown.",
            input_schema: json!({
                "type": "object",
                "properties": {"content": {"type": "string"}},
                "required": ["content"]
            }),
        },
        ToolSchema {
            name: "read_filters",
            description: "Read filters.json. Returns an empty filters object if the file does not exist.",
            input_schema: json!({"type": "object", "properties": {}}),
        },
        ToolSchema {
            name: "write_filters",
            description: "Overwrite filters.json. Accepts the full Filters object.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "title_deny": {"type": "array", "items": {"type": "string"}},
                    "location_allow_countries": {"type": "array", "items": {"type": "string"}},
                    "require_remote_or_locations": {"type": "array", "items": {"type": "string"}},
                    "clearance_deny": {"type": "array", "items": {"type": "string"}}
                }
            }),
        },
        ToolSchema {
            name: "list_known_jobs",
            description: "Recent jobs already recorded for a company, newest first.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "company": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100}
                },
                "required": ["company"]
            }),
        },
    ]
}

/// Render the tool list in the format Anthropic's `/v1/messages` expects
/// (`snake_case` `input_schema`).
pub fn to_anthropic_tools(tools: &[ToolSchema]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.input_schema,
            })
        })
        .collect()
}

/// Render the tool list in the format MCP `tools/list` expects
/// (camelCase `inputSchema`).
pub fn to_mcp_tools(tools: &[ToolSchema]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema,
            })
        })
        .collect()
}

/// Dispatch a tool call against a [`ToolKit`]. Returns the textual
/// result on success or an error message on failure; in both cases
/// the value is suitable for echoing back to the LLM as a
/// `tool_result` or MCP `tools/call` payload.
pub async fn dispatch_tool(toolkit: &ToolKit, name: &str, input: &Value) -> Result<String, String> {
    let core = &toolkit.core;
    match name {
        "fetch_url" => {
            let url = string_field(input, "url")?;
            let headers = input.get("headers").and_then(|v| v.as_object()).map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            });
            core.fetch_url(url, headers)
                .await
                .map_err(|e| e.to_string())
        }
        "save_script" => {
            let company = string_field(input, "company")?;
            let source = string_field(input, "code")?;
            core.save_script(company, source)
                .await
                .map(|()| "saved".to_string())
                .map_err(|e| e.to_string())
        }
        "run_script" => {
            let company = string_field(input, "company")?;
            let jobs = core.run_script(company).await.map_err(|e| e.to_string())?;
            serde_json::to_string(&jobs).map_err(|e| e.to_string())
        }
        "read_profile" => core.read_profile().await.map_err(|e| e.to_string()),
        "write_profile" => {
            let content = string_field(input, "content")?;
            core.write_profile(content)
                .await
                .map(|()| "written".to_string())
                .map_err(|e| e.to_string())
        }
        "read_filters" => {
            let f = core.read_filters().await.map_err(|e| e.to_string())?;
            serde_json::to_string(&f).map_err(|e| e.to_string())
        }
        "write_filters" => {
            let filters: Filters =
                serde_json::from_value(input.clone()).map_err(|e| e.to_string())?;
            core.write_filters(&filters)
                .await
                .map(|()| "written".to_string())
                .map_err(|e| e.to_string())
        }
        "list_known_jobs" => {
            let company = string_field(input, "company")?;
            let limit = input
                .get("limit")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(10) as usize;
            let jobs = core
                .list_known_jobs(company, limit)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_string(&jobs).map_err(|e| e.to_string())
        }
        other => Err(format!("unknown tool: {other}")),
    }
}

fn string_field<'a>(input: &'a Value, key: &str) -> Result<&'a str, String> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing or non-string field {key:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_tools_lists_the_documented_eight() {
        let tools = all_tools();
        let names: Vec<&str> = tools.iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            vec![
                "fetch_url",
                "save_script",
                "run_script",
                "read_profile",
                "write_profile",
                "read_filters",
                "write_filters",
                "list_known_jobs",
            ]
        );
    }

    #[test]
    fn anthropic_format_uses_snake_case_schema_key() {
        let tools = all_tools();
        let rendered = to_anthropic_tools(&tools);
        assert!(rendered[0]["input_schema"].is_object());
        assert!(rendered[0].get("inputSchema").is_none());
    }

    #[test]
    fn mcp_format_uses_camel_case_schema_key() {
        let tools = all_tools();
        let rendered = to_mcp_tools(&tools);
        assert!(rendered[0]["inputSchema"].is_object());
        assert!(rendered[0].get("input_schema").is_none());
    }
}
