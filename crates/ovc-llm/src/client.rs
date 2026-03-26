//! OpenAI-compatible HTTP client for local LLM servers.
//!
//! Supports both streaming (SSE) and non-streaming completions via the
//! `/v1/chat/completions` endpoint used by Ollama, LM Studio, and others.

use std::pin::Pin;
use std::time::Duration;

use bytes::Bytes;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};

use crate::config::ResolvedLlmConfig;
use crate::error::LlmError;

/// A message in the chat completion request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// The role: `"system"`, `"user"`, or `"assistant"`.
    pub role: String,
    /// The message content.
    pub content: String,
}

impl ChatMessage {
    /// Creates a system message.
    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_owned(),
            content: content.into(),
        }
    }

    /// Creates a user message.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_owned(),
            content: content.into(),
        }
    }
}

/// Request body for the OpenAI-compatible chat completions endpoint.
#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

/// Non-streaming response from the chat completions endpoint.
#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    content: String,
}

/// A chunk from the streaming response.
#[derive(Debug, Clone)]
pub struct StreamChunk {
    /// The incremental text content.
    pub delta: String,
    /// Whether this is the final chunk.
    pub done: bool,
}

/// Streaming SSE delta from the LLM.
#[derive(Debug, Deserialize)]
struct StreamDelta {
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamChoiceDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamChoiceDelta {
    #[serde(default)]
    content: Option<String>,
    /// Reasoning/thinking content emitted by thinking models (e.g. Qwen, DeepSeek-R1).
    #[serde(default)]
    reasoning_content: Option<String>,
}

/// Error response that LM Studio (and some other servers) embed inside SSE
/// data when the request fails (e.g. context size exceeded).
#[derive(Debug, Deserialize)]
struct SseErrorResponse {
    error: Option<SseErrorInner>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SseErrorInner {
    message: String,
}

/// Client for communicating with a local LLM via the OpenAI-compatible API.
pub struct LlmClient {
    http: reqwest::Client,
    config: ResolvedLlmConfig,
}

impl LlmClient {
    /// Creates a new LLM client with the given resolved configuration.
    pub fn new(config: ResolvedLlmConfig) -> Result<Self, LlmError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()?;

        Ok(Self { http, config })
    }

    /// Returns the configured maximum context token count.
    #[must_use]
    pub const fn max_context_tokens(&self) -> usize {
        self.config.max_context_tokens
    }

    /// The completions endpoint URL.
    fn completions_url(&self) -> String {
        let base = self.config.base_url.trim_end_matches('/');
        format!("{base}/v1/chat/completions")
    }

    /// Sends a non-streaming chat completion request and returns the full response text.
    pub async fn complete(&self, messages: Vec<ChatMessage>) -> Result<String, LlmError> {
        let request = ChatCompletionRequest {
            model: self.config.model.clone(),
            messages,
            stream: false,
            max_tokens: None,
            temperature: Some(self.config.temperature),
        };

        let mut req = self.http.post(self.completions_url()).json(&request);
        if let Some(key) = &self.config.api_key {
            req = req.bearer_auth(key);
        }

        let response = req.send().await?.error_for_status()?;
        let body: ChatCompletionResponse = response.json().await?;

        body.choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| LlmError::ParseError("no choices in response".to_owned()))
    }

    /// Sends a streaming chat completion request and returns a stream of text chunks.
    ///
    /// The returned stream yields `StreamChunk` values. The final chunk has `done: true`.
    pub async fn complete_streaming(
        &self,
        messages: Vec<ChatMessage>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk, LlmError>> + Send>>, LlmError> {
        let request = ChatCompletionRequest {
            model: self.config.model.clone(),
            messages,
            stream: true,
            max_tokens: None,
            temperature: Some(self.config.temperature),
        };

        let mut req = self.http.post(self.completions_url()).json(&request);
        if let Some(key) = &self.config.api_key {
            req = req.bearer_auth(key);
        }

        let response = req.send().await?.error_for_status()?;
        let byte_stream = response.bytes_stream();

        Ok(Box::pin(SseParser::new(byte_stream)))
    }

    /// Sends multiple non-streaming requests sequentially and returns all results.
    ///
    /// Local LLMs typically serialise inference, so sequential execution is
    /// efficient and lets callers emit progress events between batches.
    pub async fn complete_batches(
        &self,
        batches: Vec<Vec<ChatMessage>>,
    ) -> Result<Vec<String>, LlmError> {
        let mut results = Vec::with_capacity(batches.len());
        for messages in batches {
            results.push(self.complete(messages).await?);
        }
        Ok(results)
    }

    /// Checks whether the LLM server is reachable by hitting the models endpoint.
    ///
    /// Returns `Ok(true)` if reachable, `Ok(false)` if not, or an error on
    /// unexpected failures.
    pub async fn health_check(&self) -> Result<bool, LlmError> {
        let base = self.config.base_url.trim_end_matches('/');
        let url = format!("{base}/v1/models");

        let mut req = self.http.get(&url);
        if let Some(key) = &self.config.api_key {
            req = req.bearer_auth(key);
        }

        match req.send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(e) if e.is_connect() || e.is_timeout() => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
}

/// Internal SSE parser that converts a byte stream into `StreamChunk` items.
///
/// Handles:
/// - Standard `OpenAI` `data: {json}` lines with `[DONE]` terminator
/// - SSE `event:` field tracking (recognises `event: error` from LM Studio et al.)
/// - Inline error JSON (`{"error": {"message": "..."}}`), returned by LM Studio
///   when e.g. context size is exceeded (HTTP 200 + SSE error event)
/// - `reasoning_content` deltas from thinking models (Qwen, DeepSeek-R1, etc.)
struct SseParser<S> {
    inner: S,
    buffer: String,
    /// Tracks the most recently seen `event:` value within the current SSE block.
    current_event: Option<String>,
    /// Once we have yielded a terminal error, the stream is done.
    finished: bool,
}

impl<S> SseParser<S> {
    const fn new(inner: S) -> Self {
        Self {
            inner,
            buffer: String::new(),
            current_event: None,
            finished: false,
        }
    }
}

/// Try to extract a human-readable error message from JSON that LM Studio /
/// other servers embed in SSE `data:` lines on failure.
fn try_parse_sse_error(data: &str) -> Option<String> {
    let resp: SseErrorResponse = serde_json::from_str(data).ok()?;
    resp.error.map(|e| e.message).or(resp.message)
}

/// Extract the useful text content from a successfully parsed `StreamDelta`.
///
/// Only uses `content` — `reasoning_content` (thinking-model chain-of-thought)
/// is intentionally discarded as it's internal reasoning, not the final answer.
fn extract_chunk(delta: &StreamDelta) -> (String, bool) {
    let choice = delta.choices.first();
    let content = choice
        .and_then(|c| c.delta.content.as_deref())
        .unwrap_or_default()
        .to_owned();
    let done = choice.and_then(|c| c.finish_reason.as_ref()).is_some();
    (content, done)
}

impl<S> Stream for SseParser<S>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
{
    type Item = Result<StreamChunk, LlmError>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if this.finished {
            return std::task::Poll::Ready(None);
        }

        loop {
            // Try to parse a complete SSE line from the buffer.
            if let Some(newline_pos) = this.buffer.find('\n') {
                let line = this.buffer[..newline_pos].trim().to_owned();
                this.buffer.drain(..=newline_pos);

                // Empty line = end of SSE event block; reset event type.
                if line.is_empty() {
                    this.current_event = None;
                    continue;
                }

                // SSE comment lines (`:keep-alive`, etc.) — skip.
                if line.starts_with(':') {
                    continue;
                }

                // Track `event:` field.
                if let Some(event) = line.strip_prefix("event:") {
                    this.current_event = Some(event.trim().to_owned());
                    continue;
                }

                // Process `data:` lines.
                if let Some(data) = line.strip_prefix("data:") {
                    let data = data.trim();

                    if data == "[DONE]" {
                        this.finished = true;
                        return std::task::Poll::Ready(Some(Ok(StreamChunk {
                            delta: String::new(),
                            done: true,
                        })));
                    }

                    // If the server told us this is an error event, surface it.
                    if this.current_event.as_deref() == Some("error") {
                        this.finished = true;
                        let msg = try_parse_sse_error(data).unwrap_or_else(|| data.to_owned());
                        return std::task::Poll::Ready(Some(Err(LlmError::ParseError(msg))));
                    }

                    // Try to parse as a normal streaming delta.
                    if let Ok(delta) = serde_json::from_str::<StreamDelta>(data) {
                        let (content, done) = extract_chunk(&delta);
                        if !content.is_empty() || done {
                            return std::task::Poll::Ready(Some(Ok(StreamChunk {
                                delta: content,
                                done,
                            })));
                        }
                    } else {
                        // Not a valid delta — check if it's an inline error
                        // (LM Studio sends errors as data without event: error).
                        if let Some(msg) = try_parse_sse_error(data) {
                            this.finished = true;
                            return std::task::Poll::Ready(Some(Err(LlmError::ParseError(msg))));
                        }
                        tracing::debug!("skipping unparseable SSE data: {data}");
                    }
                    continue;
                }
                // Unknown SSE field — skip.
                continue;
            }

            // Need more data from the underlying stream.
            match Pin::new(&mut this.inner).poll_next(cx) {
                std::task::Poll::Ready(Some(Ok(bytes))) => {
                    this.buffer.push_str(&String::from_utf8_lossy(&bytes));
                }
                std::task::Poll::Ready(Some(Err(e))) => {
                    this.finished = true;
                    return std::task::Poll::Ready(Some(Err(e.into())));
                }
                std::task::Poll::Ready(None) => {
                    // Stream ended. If there's leftover data, try to parse it.
                    if !this.buffer.is_empty() {
                        let remaining = std::mem::take(&mut this.buffer);
                        let data = remaining.trim();
                        if let Some(data) = data.strip_prefix("data:") {
                            let data = data.trim();
                            if data != "[DONE]" {
                                // Check for error first.
                                if let Some(msg) = try_parse_sse_error(data) {
                                    this.finished = true;
                                    return std::task::Poll::Ready(Some(Err(
                                        LlmError::ParseError(msg),
                                    )));
                                }
                                if let Ok(delta) = serde_json::from_str::<StreamDelta>(data) {
                                    let (content, _) = extract_chunk(&delta);
                                    return std::task::Poll::Ready(Some(Ok(StreamChunk {
                                        delta: content,
                                        done: true,
                                    })));
                                }
                            }
                        }
                    }
                    return std::task::Poll::Ready(None);
                }
                std::task::Poll::Pending => {
                    return std::task::Poll::Pending;
                }
            }
        }
    }
}
