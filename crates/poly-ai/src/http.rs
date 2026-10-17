//! The only network code: a blocking HTTP client that talks to a loopback
//! OpenAI-compatible server and nothing else.
//!
//! Proxies are off (environment variables are never read), redirects are
//! not followed, and every URL passes [`crate::guard::validate_base_url`]
//! right before the request. Request and response bodies are never logged.

use crate::AiError;
use crate::guard::validate_base_url;
use serde_json::{Value, json};
use std::time::Duration;

/// Largest response body we read. A model list or one short chat answer is
/// a few KB; 1 MiB is generous and stops a misbehaving server from filling memory.
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;
/// HTTP 200 OK. Anything else (including redirects, which we never follow) is an error.
const HTTP_OK: u16 = 200;

pub struct Http {
    agent: ureq::Agent,
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

impl Http {
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            // Never use a proxy, and never read HTTP_PROXY/HTTPS_PROXY/ALL_PROXY
            // (ureq's default reads them).
            .proxy(None)
            // 0 = don't follow redirects; the 3xx response comes back to us
            // and is rejected as a non-200 status.
            .max_redirects(0)
            // Handle every status ourselves.
            .http_status_as_error(false)
            .build();
        Http {
            agent: config.into(),
        }
    }

    fn read_ok(
        response: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<Value, AiError> {
        let mut response = response.map_err(map_error)?;
        let status = response.status().as_u16();
        if status != HTTP_OK {
            return Err(AiError::Server(format!("HTTP status {status}")));
        }
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BYTES)
            .read_to_string()
            .map_err(map_error)?;
        // Never put the body in an error: it may contain model output.
        serde_json::from_str(&body).map_err(|_| AiError::BadResponse)
    }

    /// `GET {base}/models` → model ids.
    pub fn list_models(&self, base_url: &str, timeout: Duration) -> Result<Vec<String>, AiError> {
        let base = validate_base_url(base_url)?;
        let v = Self::read_ok(
            self.agent
                .get(format!("{base}/models"))
                .config()
                .timeout_global(Some(timeout))
                .build()
                .call(),
        )?;
        let data = v["data"].as_array().ok_or(AiError::BadResponse)?;
        Ok(data
            .iter()
            .filter_map(|m| m["id"].as_str().map(str::to_string))
            .collect())
    }

    /// `POST {base}/chat/completions` (non-streaming) → the answer text.
    pub fn chat(
        &self,
        base_url: &str,
        request: &ChatRequest,
        timeout: Duration,
    ) -> Result<String, AiError> {
        let ChatRequest {
            model,
            system,
            user,
            temperature,
            max_tokens,
        } = request;
        let base = validate_base_url(base_url)?;
        let body = json!({
            "model": model,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user },
            ],
            "temperature": temperature,
            "max_tokens": max_tokens,
            "stream": false,
        });
        let v = Self::read_ok(
            self.agent
                .post(format!("{base}/chat/completions"))
                .header("Content-Type", "application/json")
                .config()
                .timeout_global(Some(timeout))
                .build()
                .send(body.to_string()),
        )?;
        v["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_string)
            .ok_or(AiError::BadResponse)
    }
}

/// One chat request (system + user message).
pub struct ChatRequest<'a> {
    pub model: &'a str,
    pub system: &'a str,
    pub user: &'a str,
    pub temperature: f64,
    pub max_tokens: u32,
}

fn map_error(e: ureq::Error) -> AiError {
    match e {
        ureq::Error::Timeout(_) => AiError::Timeout,
        ureq::Error::Io(ref io) if io.kind() == std::io::ErrorKind::TimedOut => AiError::Timeout,
        ureq::Error::Io(ref io)
            if matches!(
                io.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::AddrNotAvailable
            ) =>
        {
            AiError::NoLocalServer
        }
        ureq::Error::ConnectionFailed => AiError::NoLocalServer,
        ureq::Error::BodyExceedsLimit(_) => AiError::BadResponse,
        other => AiError::Server(other.to_string()),
    }
}
