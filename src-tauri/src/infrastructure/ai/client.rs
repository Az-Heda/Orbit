//! Claude Messages API over `reqwest` + `serde` (there is no official Rust SDK).
//! Streams server-sent events and assembles the content blocks, keeping thinking and tool_use
//! blocks verbatim so they can be replayed on the next request.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{Value, json};

use super::secrets::SecretStore;

pub const API_VERSION: &str = "2023-06-01";
const DEFAULT_BASE: &str = "https://api.anthropic.com";
const MAX_TOKENS: u32 = 16_000;
const MAX_RETRIES: u32 = 2;

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("no API key configured; add one in Settings")]
    MissingKey,
    #[error("the API key was rejected; check it in Settings")]
    Auth,
    #[error("rate limited by the Claude API; try again in a moment")]
    RateLimited,
    #[error("Claude API error ({status}): {message}")]
    Api { status: u16, message: String },
    #[error("network error: {0}")]
    Network(String),
    #[error("unexpected response from the Claude API: {0}")]
    Protocol(String),
    #[error("cancelled")]
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct ApiRequest {
    pub model: String,
    /// System prompt blocks (already includes any cache_control).
    pub system: Vec<Value>,
    pub messages: Vec<Value>,
    pub tools: Vec<Value>,
    pub effort: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    Text(String),
    /// A tool call started streaming (name only; arguments are not shown).
    ToolStarted(String),
}

#[derive(Debug, Clone, Default)]
pub struct ModelReply {
    /// Content blocks in order, exactly as they must be echoed back.
    pub content: Vec<Value>,
    /// end_turn | tool_use | max_tokens | refusal | stop_sequence | pause_turn
    pub stop_reason: String,
    pub refusal_category: Option<String>,
    /// tool_use ids whose streamed arguments were not valid JSON.
    pub invalid_tool_ids: Vec<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

pub type Sink<'a> = &'a mut (dyn FnMut(StreamEvent) + Send);

#[async_trait]
pub trait ModelClient: Send + Sync {
    async fn stream(
        &self,
        req: &ApiRequest,
        sink: Sink<'_>,
        cancel: &AtomicBool,
    ) -> Result<ModelReply, AiError>;
}

/// Models that take adaptive thinking and `output_config.effort`.
fn supports_adaptive(model: &str) -> bool {
    [
        "fable-5",
        "mythos-5",
        "opus-5",
        "sonnet-5",
        "haiku-5",
        "opus-4-6",
        "opus-4-7",
        "opus-4-8",
        "sonnet-4-6",
    ]
    .iter()
    .any(|m| model.contains(m))
}

pub fn build_body(req: &ApiRequest) -> Value {
    let mut body = json!({
        "model": req.model,
        "max_tokens": MAX_TOKENS,
        "stream": true,
        "system": req.system,
        "messages": req.messages,
    });
    if !req.tools.is_empty() {
        body["tools"] = Value::Array(req.tools.clone());
    }
    if supports_adaptive(&req.model) {
        body["thinking"] = json!({ "type": "adaptive" });
        body["output_config"] = json!({ "effort": req.effort });
    }
    body
}

// ---------- SSE ----------

/// Incremental server-sent-events splitter: feed bytes, get complete `(event, data)` frames.
#[derive(Default)]
pub struct SseParser {
    buf: String,
    pending: Vec<u8>,
}

impl SseParser {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<(String, String)> {
        self.pending.extend_from_slice(bytes);
        // Only decode up to the last complete UTF-8 boundary.
        let valid = match std::str::from_utf8(&self.pending) {
            Ok(s) => s.len(),
            Err(e) => e.valid_up_to(),
        };
        let chunk: Vec<u8> = self.pending.drain(..valid).collect();
        self.buf
            .push_str(&String::from_utf8_lossy(&chunk).replace("\r\n", "\n"));
        let mut out = vec![];
        while let Some(i) = self.buf.find("\n\n") {
            let frame: String = self.buf.drain(..i + 2).collect();
            let (mut event, mut data) = (String::new(), String::new());
            for line in frame.lines() {
                if let Some(v) = line.strip_prefix("event:") {
                    event = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("data:") {
                    if !data.is_empty() {
                        data.push('\n');
                    }
                    data.push_str(v.strip_prefix(' ').unwrap_or(v));
                }
            }
            if !data.is_empty() {
                out.push((event, data));
            }
        }
        out
    }
}

/// Builds the reply from stream events.
#[derive(Default)]
pub struct Accumulator {
    blocks: Vec<Option<Value>>,
    partial: Vec<String>,
    reply: ModelReply,
    done: bool,
}

impl Accumulator {
    pub fn feed(&mut self, data: &str, sink: Sink<'_>) -> Result<(), AiError> {
        let ev: Value =
            serde_json::from_str(data).map_err(|e| AiError::Protocol(format!("bad event: {e}")))?;
        let idx = ev.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        match ev.get("type").and_then(Value::as_str).unwrap_or("") {
            "message_start" => {
                if let Some(n) = ev
                    .pointer("/message/usage/input_tokens")
                    .and_then(Value::as_u64)
                {
                    self.reply.input_tokens = n;
                }
            }
            "content_block_start" => {
                if self.blocks.len() <= idx {
                    self.blocks.resize(idx + 1, None);
                    self.partial.resize(idx + 1, String::new());
                }
                let block = ev.get("content_block").cloned().unwrap_or(Value::Null);
                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    let name = block.get("name").and_then(Value::as_str).unwrap_or("");
                    sink(StreamEvent::ToolStarted(name.to_string()));
                }
                self.blocks[idx] = Some(block);
            }
            "content_block_delta" => {
                let Some(Some(block)) = self.blocks.get_mut(idx) else {
                    return Err(AiError::Protocol("delta for unknown block".into()));
                };
                let delta = &ev["delta"];
                match delta.get("type").and_then(Value::as_str).unwrap_or("") {
                    "text_delta" => {
                        let t = delta.get("text").and_then(Value::as_str).unwrap_or("");
                        let cur = block["text"].as_str().unwrap_or("").to_string();
                        block["text"] = Value::String(cur + t);
                        sink(StreamEvent::Text(t.to_string()));
                    }
                    "input_json_delta" => {
                        let j = delta
                            .get("partial_json")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        self.partial[idx].push_str(j);
                    }
                    "thinking_delta" => {
                        let t = delta.get("thinking").and_then(Value::as_str).unwrap_or("");
                        let cur = block["thinking"].as_str().unwrap_or("").to_string();
                        block["thinking"] = Value::String(cur + t);
                    }
                    "signature_delta" => {
                        let t = delta.get("signature").and_then(Value::as_str).unwrap_or("");
                        let cur = block["signature"].as_str().unwrap_or("").to_string();
                        block["signature"] = Value::String(cur + t);
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                if let Some(Some(block)) = self.blocks.get_mut(idx)
                    && block.get("type").and_then(Value::as_str) == Some("tool_use")
                {
                    let raw = self.partial[idx].trim();
                    if raw.is_empty() {
                        block["input"] = json!({});
                    } else {
                        match serde_json::from_str::<Value>(raw) {
                            Ok(v) => block["input"] = v,
                            Err(_) => {
                                block["input"] = json!({});
                                if let Some(id) = block.get("id").and_then(Value::as_str) {
                                    self.reply.invalid_tool_ids.push(id.to_string());
                                }
                            }
                        }
                    }
                }
            }
            "message_delta" => {
                if let Some(r) = ev.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.reply.stop_reason = r.to_string();
                }
                if let Some(c) = ev
                    .pointer("/delta/stop_details/category")
                    .and_then(Value::as_str)
                {
                    self.reply.refusal_category = Some(c.to_string());
                }
                if let Some(n) = ev.pointer("/usage/output_tokens").and_then(Value::as_u64) {
                    self.reply.output_tokens = n;
                }
            }
            "message_stop" => self.done = true,
            "error" => {
                let msg = ev
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("stream error")
                    .to_string();
                let overloaded =
                    ev.pointer("/error/type").and_then(Value::as_str) == Some("overloaded_error");
                return Err(AiError::Api {
                    status: if overloaded { 529 } else { 500 },
                    message: msg,
                });
            }
            _ => {}
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<ModelReply, AiError> {
        if !self.done || self.reply.stop_reason.is_empty() {
            return Err(AiError::Protocol("the stream ended early".into()));
        }
        self.reply.content = self.blocks.into_iter().flatten().collect();
        Ok(self.reply)
    }
}

// ---------- HTTP client ----------

pub struct ClaudeClient {
    http: reqwest::Client,
    base: String,
    secrets: Arc<dyn SecretStore>,
}

impl ClaudeClient {
    pub fn new(secrets: Arc<dyn SecretStore>) -> Self {
        Self::with_base(secrets, DEFAULT_BASE)
    }

    pub fn with_base(secrets: Arc<dyn SecretStore>, base: &str) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            // Streams can run for minutes; guard against a stalled connection instead.
            .read_timeout(Duration::from_secs(120))
            .build()
            .expect("http client");
        Self {
            http,
            base: base.trim_end_matches('/').to_string(),
            secrets,
        }
    }
}

fn api_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "request failed".into())
}

#[async_trait]
impl ModelClient for ClaudeClient {
    async fn stream(
        &self,
        req: &ApiRequest,
        sink: Sink<'_>,
        cancel: &AtomicBool,
    ) -> Result<ModelReply, AiError> {
        let key = self
            .secrets
            .get()
            .map_err(|e| AiError::Network(e.to_string()))?
            .ok_or(AiError::MissingKey)?;
        let body = build_body(req);

        let mut attempt = 0;
        let resp = loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(AiError::Cancelled);
            }
            let sent = self
                .http
                .post(format!("{}/v1/messages", self.base))
                .header("x-api-key", &key)
                .header("anthropic-version", API_VERSION)
                .json(&body)
                .send()
                .await;
            let retry_after = match sent {
                Ok(r) if r.status().is_success() => break r,
                Ok(r) => {
                    let status = r.status().as_u16();
                    let wait = r
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(1u64 << attempt)
                        .min(10);
                    let text = r.text().await.unwrap_or_default();
                    let retryable = matches!(status, 429 | 500 | 502 | 503 | 504 | 529);
                    if !retryable || attempt >= MAX_RETRIES {
                        return Err(match status {
                            401 | 403 => AiError::Auth,
                            429 => AiError::RateLimited,
                            _ => AiError::Api {
                                status,
                                message: api_message(&text),
                            },
                        });
                    }
                    wait
                }
                Err(e) => {
                    if attempt >= MAX_RETRIES {
                        return Err(AiError::Network(e.without_url().to_string()));
                    }
                    1u64 << attempt
                }
            };
            attempt += 1;
            tokio::time::sleep(Duration::from_secs(retry_after)).await;
        };

        let mut parser = SseParser::default();
        let mut acc = Accumulator::default();
        let mut bytes = resp.bytes_stream();
        while let Some(chunk) = bytes.next().await {
            if cancel.load(Ordering::Relaxed) {
                return Err(AiError::Cancelled);
            }
            let chunk = chunk.map_err(|e| AiError::Network(e.without_url().to_string()))?;
            for (_event, data) in parser.feed(&chunk) {
                acc.feed(&data, sink)?;
            }
        }
        acc.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SSE: &str = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":12}}}\n\n\
event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\",\"signature\":\"\"}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"abc\"}}\n\n\
event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hel\"}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"lo é\"}}\n\n\
event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n\
event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"get_tasks\",\"input\":{}}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"limit\\\":\"}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"5}\"}}\n\n\
event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":2}\n\n\
event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":30}}\n\n\
event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";

    fn run(chunks: Vec<&[u8]>) -> Result<(ModelReply, Vec<StreamEvent>), AiError> {
        let mut parser = SseParser::default();
        let mut acc = Accumulator::default();
        let mut events = vec![];
        let mut sink = |e| events.push(e);
        for c in chunks {
            for (_, d) in parser.feed(c) {
                acc.feed(&d, &mut sink)?;
            }
        }
        Ok((acc.finish()?, events))
    }

    #[test]
    fn assembles_blocks_across_arbitrary_chunk_boundaries() {
        // Split inside a multi-byte character and mid-frame.
        let bytes = SSE.as_bytes();
        let cut = SSE.find('é').unwrap() + 1;
        let (reply, events) = run(vec![
            &bytes[..cut],
            &bytes[cut..cut + 40],
            &bytes[cut + 40..],
        ])
        .unwrap();
        assert_eq!(reply.stop_reason, "tool_use");
        assert_eq!(reply.content.len(), 3);
        assert_eq!(
            reply.content[0]["signature"], "abc",
            "thinking block is kept verbatim"
        );
        assert_eq!(reply.content[1]["text"], "Hello é");
        assert_eq!(reply.content[2]["input"], json!({"limit": 5}));
        assert_eq!((reply.input_tokens, reply.output_tokens), (12, 30));
        assert_eq!(
            events,
            vec![
                StreamEvent::Text("Hel".into()),
                StreamEvent::Text("lo é".into()),
                StreamEvent::ToolStarted("get_tasks".into())
            ]
        );
    }

    #[test]
    fn invalid_tool_json_is_reported_not_run() {
        let sse = "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"x\",\"input\":{}}}\n\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"a\\\":\"}}\n\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"}}\n\n\
data: {\"type\":\"message_stop\"}\n\n";
        let (reply, _) = run(vec![sse.as_bytes()]).unwrap();
        assert_eq!(reply.invalid_tool_ids, vec!["t".to_string()]);
        assert_eq!(reply.stop_reason, "max_tokens");
    }

    #[test]
    fn refusal_category_and_early_end() {
        let sse = "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"refusal\",\"stop_details\":{\"type\":\"refusal\",\"category\":\"cyber\"}}}\n\ndata: {\"type\":\"message_stop\"}\n\n";
        let (reply, _) = run(vec![sse.as_bytes()]).unwrap();
        assert_eq!(reply.refusal_category.as_deref(), Some("cyber"));
        assert!(matches!(
            run(vec![
                b"data: {\"type\":\"message_start\",\"message\":{}}\n\n"
            ]),
            Err(AiError::Protocol(_))
        ));
        let err = "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"busy\"}}\n\n";
        assert!(matches!(
            run(vec![err.as_bytes()]),
            Err(AiError::Api { status: 529, .. })
        ));
    }

    #[test]
    fn body_gates_thinking_and_effort_by_model() {
        let mut req = ApiRequest {
            model: "claude-opus-5-5".into(),
            system: vec![json!({"type":"text","text":"s"})],
            messages: vec![json!({"role":"user","content":"hi"})],
            tools: vec![],
            effort: "medium".into(),
        };
        let b = build_body(&req);
        assert_eq!(b["thinking"]["type"], "adaptive");
        assert_eq!(b["output_config"]["effort"], "medium");
        assert_eq!(b["stream"], true);
        assert!(b.get("tools").is_none() && b.get("tool_choice").is_none());
        assert!(b.get("temperature").is_none());
        req.model = "claude-haiku-4-5".into();
        let b = build_body(&req);
        assert!(b.get("thinking").is_none() && b.get("output_config").is_none());
    }

    #[tokio::test]
    async fn streams_from_a_local_server_and_hides_the_key_from_errors() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(std::sync::Mutex::new(String::new()));
        let seen2 = seen.clone();
        tokio::spawn(async move {
            for round in 0..2 {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 65536];
                let n = sock.read(&mut buf).await.unwrap();
                *seen2.lock().unwrap() = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                let resp = if round == 0 {
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        SSE.len(),
                        SSE
                    )
                } else {
                    let body =
                        "{\"error\":{\"type\":\"authentication_error\",\"message\":\"nope\"}}";
                    format!(
                        "HTTP/1.1 401 Unauthorized\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                };
                sock.write_all(resp.as_bytes()).await.unwrap();
            }
        });
        let secrets = Arc::new(super::super::secrets::MemorySecretStore::default());
        let client = ClaudeClient::with_base(secrets.clone(), &format!("http://{addr}"));
        let req = ApiRequest {
            model: "claude-opus-5-5".into(),
            system: vec![],
            messages: vec![json!({"role":"user","content":"hi"})],
            tools: vec![],
            effort: "low".into(),
        };
        let cancel = AtomicBool::new(false);
        let mut sink = |_e: StreamEvent| {};
        assert!(matches!(
            client.stream(&req, &mut sink, &cancel).await,
            Err(AiError::MissingKey)
        ));
        secrets.set("sk-ant-test-123").unwrap();
        let reply = client.stream(&req, &mut sink, &cancel).await.unwrap();
        assert_eq!(reply.stop_reason, "tool_use");
        let head = seen.lock().unwrap().clone();
        assert!(
            head.contains("x-api-key: sk-ant-test-123")
                && head.contains("anthropic-version: 2023-06-01")
        );
        assert!(head.contains("post /v1/messages"));
        let err = client.stream(&req, &mut sink, &cancel).await.unwrap_err();
        assert!(matches!(err, AiError::Auth));
        assert!(!err.to_string().contains("sk-ant"));
    }
}
