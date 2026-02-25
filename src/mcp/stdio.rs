#![cfg(feature = "mcp")]

use super::McpServerConfig;
use super::tools::McpTool;
use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn list_tools(server: &McpServerConfig) -> anyhow::Result<Vec<McpTool>> {
    let mut rpc = StdioRpc::spawn(server).await?;

    // MCP initialize
    let init = rpc
        .request::<serde_json::Value, InitializeResult>(
            "initialize",
            InitializeParams {
                protocol_version: "2024-11-05".to_string(),
                capabilities: serde_json::json!({}),
                client_info: ClientInfo {
                    name: env!("CARGO_PKG_NAME").to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                },
            },
        )
        .await
        .context("initialize failed")?;

    let _ = init;

    let tools = rpc
        .request::<serde_json::Value, ToolsListResult>("tools/list", serde_json::json!({}))
        .await
        .context("tools/list failed")?;

    Ok(tools.tools)
}

#[derive(Debug, Clone, Serialize)]
struct InitializeParams {
    #[serde(rename = "protocolVersion")]
    protocol_version: String,
    capabilities: serde_json::Value,
    #[serde(rename = "clientInfo")]
    client_info: ClientInfo,
}

#[derive(Debug, Clone, Serialize)]
struct ClientInfo {
    name: String,
    version: String,
}

#[derive(Debug, Clone, Deserialize)]
struct InitializeResult {
    #[allow(dead_code)]
    #[serde(default)]
    capabilities: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct ToolsListResult {
    #[serde(default)]
    tools: Vec<McpTool>,
}

#[derive(Debug)]
struct StdioRpc {
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
    next_id: u64,
}

impl StdioRpc {
    async fn spawn(server: &McpServerConfig) -> anyhow::Result<Self> {
        let mut cmd = tokio::process::Command::new(&server.command);
        cmd.args(&server.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());

        let mut child = cmd.spawn().with_context(|| {
            format!(
                "failed to spawn MCP server: {} {:?}",
                server.command, server.args
            )
        })?;

        let stdin = child.stdin.take().context("child stdin missing")?;
        let stdout = child.stdout.take().context("child stdout missing")?;

        Ok(Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        })
    }

    async fn request<P: Serialize, R: for<'de> Deserialize<'de>>(
        &mut self,
        method: &str,
        params: P,
    ) -> anyhow::Result<R> {
        let id = self.next_id;
        self.next_id += 1;

        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method,
            params,
        };

        self.write_message(&req).await?;

        loop {
            let raw = self.read_message().await?;
            let v: serde_json::Value = serde_json::from_slice(&raw).context("invalid JSON-RPC")?;

            // Try decode success.
            if v.get("id").and_then(|x| x.as_u64()) != Some(id) {
                continue;
            }

            if v.get("error").is_some() {
                let err: JsonRpcErrorEnvelope = serde_json::from_value(v)
                    .context("invalid error envelope")?;
                return Err(anyhow!(
                    "MCP error {}: {}",
                    err.error.code,
                    err.error.message
                ));
            }

            let ok: JsonRpcOkEnvelope<R> = serde_json::from_value(v).context("invalid ok envelope")?;
            return Ok(ok.result);
        }
    }

    async fn write_message<T: Serialize>(&mut self, msg: &T) -> anyhow::Result<()> {
        let body = serde_json::to_vec(msg).context("failed to encode JSON")?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.stdin
            .write_all(header.as_bytes())
            .await
            .context("failed to write header")?;
        self.stdin
            .write_all(&body)
            .await
            .context("failed to write body")?;
        self.stdin.flush().await.ok();
        Ok(())
    }

    async fn read_message(&mut self) -> anyhow::Result<Vec<u8>> {
        // Read headers until CRLF CRLF.
        let mut header_buf = Vec::new();
        let mut tmp = [0u8; 1];
        loop {
            let n = self.stdout.read(&mut tmp).await.context("read header")?;
            if n == 0 {
                return Err(anyhow!("MCP server closed stdout"));
            }
            header_buf.push(tmp[0]);
            if header_buf.ends_with(b"\r\n\r\n") {
                break;
            }
            if header_buf.len() > 8192 {
                return Err(anyhow!("header too large"));
            }
        }

        let header_str = std::str::from_utf8(&header_buf).context("header not UTF-8")?;
        let mut content_len: Option<usize> = None;
        for line in header_str.split("\r\n") {
            let Some((k, v)) = line.split_once(":") else { continue; };
            if k.eq_ignore_ascii_case("content-length") {
                content_len = Some(v.trim().parse::<usize>().context("bad Content-Length")?);
            }
        }
        let len = content_len.context("missing Content-Length")?;

        let mut body = vec![0u8; len];
        self.stdout
            .read_exact(&mut body)
            .await
            .context("read body")?;
        Ok(body)
    }
}

#[derive(Debug, Serialize)]
struct JsonRpcRequest<'a, P> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: P,
}

#[derive(Debug, Deserialize)]
struct JsonRpcOkEnvelope<R> {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: u64,
    result: R,
}

#[derive(Debug, Deserialize)]
struct JsonRpcErrorEnvelope {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: u64,
    error: JsonRpcError,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}
