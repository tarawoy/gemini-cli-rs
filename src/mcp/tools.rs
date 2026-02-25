#![cfg(feature = "mcp")]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct RegisteredTool {
    pub server: String,
    pub name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Default)]
pub struct ToolRegistry {
    tools: Vec<RegisteredTool>,
}

impl ToolRegistry {
    pub fn register_server_tools(&mut self, server: &str, tools: Vec<McpTool>) {
        for t in tools {
            self.tools.push(RegisteredTool {
                server: server.to_string(),
                name: t.name,
                description: t.description,
                input_schema: t.input_schema,
            });
        }
    }

    pub fn list(&self) -> &[RegisteredTool] {
        &self.tools
    }
}
