//! The provider boundary: one small interface, an adapter per provider, and the only network
//! access in the workspace.
//!
//! A [`Provider`] sends one independent request — a system text and one user message, no
//! tools, no conversation — and returns the reply's text, the model identifier the provider
//! answered with, and its usage both raw and normalised. It never scores, prices or retries;
//! those are the runner's. An adapter reaches its API only through a [`Transport`], so every
//! test drives it with recorded responses; [`Curl`] is the one real transport, and passes the
//! credential to `curl` on standard input, never in its argument list.

use std::cell::Cell;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

use crate::price::Usage;
use crate::prompt::Request;

/// A reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub text: String,
    /// The model identifier the provider answered with.
    pub model: String,
    pub stop_reason: Option<String>,
    pub usage: Usage,
    /// The provider's own usage object.
    pub raw_usage: Value,
    /// A provider's backend fingerprint, where it reports one.
    pub fingerprint: Option<String>,
    /// The whole response body, for the local evidence directory.
    pub raw: String,
}

/// Why a call returned no reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// Transient — a timeout, a rate limit, an overloaded or failing server. The runner may
    /// retry it, a bounded number of times.
    Retryable { reason: String, billed: bool },
    /// Not transient — a malformed request, a refused credential, an unknown model.
    Fatal { reason: String },
}

/// The interface every provider implements.
pub trait Provider {
    /// `openai`, `anthropic`, `fake`.
    fn name(&self) -> &str;
    /// The model identifier requested.
    fn model(&self) -> &str;
    /// Every request parameter except the messages, as sent: part of each request's identity.
    fn parameters(&self) -> Value;
    fn max_output_tokens(&self) -> u64;
    /// One request.
    ///
    /// # Errors
    ///
    /// When there is no reply.
    fn complete(&self, request: &Request) -> Result<Reply, CallError>;
    /// The provider's own count of a request's input, where it offers one without generating.
    ///
    /// # Errors
    ///
    /// When the count was asked for and not answered.
    fn count(&self, request: &Request) -> Result<Option<u64>, CallError>;
    /// The factor, as a fraction, that bounds this provider's input tokens above the local
    /// `o200k_base` count, for the worst-case cost a request is admitted under.
    fn input_bound(&self) -> (u64, u64);
}

/// An HTTP response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Http {
    pub status: u16,
    pub body: String,
}

/// How an adapter reaches its API.
pub trait Transport {
    /// One POST.
    ///
    /// # Errors
    ///
    /// When there is no HTTP response at all.
    fn post(&self, url: &str, headers: &[(&str, String)], body: &str) -> Result<Http, String>;
}

/// A credential. It prints as `[redacted]`, is never serialised, and is removed from any
/// error text before that text is kept.
pub struct Secret(String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret([redacted])")
    }
}

impl Secret {
    /// From `var`, or else from `~/.config/nazm-agent-bench/<provider>.key`, which must be
    /// readable by its owner only.
    ///
    /// # Errors
    ///
    /// When neither has a credential, the file is readable by others, or the value has a
    /// character no API key has. The message never contains the value.
    pub fn load(var: &str, provider: &str) -> Result<Self, String> {
        let value = match std::env::var(var) {
            Ok(v) if !v.trim().is_empty() => v,
            _ => {
                let path = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_default()
                    .join(".config/nazm-agent-bench")
                    .join(format!("{provider}.key"));
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt as _;
                    if let Ok(m) = std::fs::metadata(&path) {
                        if m.permissions().mode() & 0o077 != 0 {
                            return Err(format!(
                                "{} is readable by others; make it 0600",
                                path.display()
                            ));
                        }
                    }
                }
                std::fs::read_to_string(&path).map_err(|_| {
                    format!(
                        "no credential for {provider}: set {var}, or put it in {}",
                        path.display()
                    )
                })?
            }
        };
        let value = value.trim().to_owned();
        if value.is_empty()
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        {
            return Err(format!(
                "the {provider} credential has a character no API key has"
            ));
        }
        Ok(Self(value))
    }

    /// A secret for tests.
    #[must_use]
    pub fn new(value: &str) -> Self {
        Self(value.to_owned())
    }

    /// `text` with the credential removed.
    #[must_use]
    pub fn redact(&self, text: &str) -> String {
        if self.0.is_empty() {
            text.to_owned()
        } else {
            text.replace(&self.0, "[redacted]")
        }
    }
}

/// `curl`, with the headers — the credential among them — on standard input as its config,
/// and the body in a temporary file.
#[derive(Debug, Clone, Copy)]
pub struct Curl {
    pub timeout_seconds: u32,
}

impl Transport for Curl {
    fn post(&self, url: &str, headers: &[(&str, String)], body: &str) -> Result<Http, String> {
        let mut file = tempfile::NamedTempFile::new().map_err(|e| format!("a body file: {e}"))?;
        file.write_all(body.as_bytes())
            .and_then(|()| file.flush())
            .map_err(|e| format!("a body file: {e}"))?;
        let mut config = String::new();
        for (k, v) in headers {
            if v.contains(['"', '\\', '\n', '\r']) || k.contains(['"', '\\', '\n', '\r']) {
                return Err("a header has a character curl's config cannot carry".to_owned());
            }
            config.push_str(&format!("header = \"{k}: {v}\"\n"));
        }
        let mut child = Command::new("curl")
            .args(["--silent", "--show-error", "--max-time"])
            .arg(self.timeout_seconds.to_string())
            .args(["--request", "POST", "--config", "-", "--data-binary"])
            .arg(format!("@{}", file.path().display()))
            .args(["--write-out", "\n%{http_code}", "--url", url])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("curl could not be started: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(config.as_bytes())
                .map_err(|e| format!("curl's config: {e}"))?;
        }
        let out = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "curl exited with {}: {}",
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let (body, code) = text.rsplit_once('\n').unwrap_or(("", &text));
        let status = code
            .trim()
            .parse()
            .map_err(|_| format!("curl reported no status: {code:?}"))?;
        Ok(Http {
            status,
            body: body.to_owned(),
        })
    }
}

fn clip(s: &str) -> String {
    s.chars().take(600).collect()
}

/// A response that is not a reply: transient or not by its status.
fn refused(http: &Http, key: &Secret) -> CallError {
    let reason = format!("HTTP {}: {}", http.status, clip(&key.redact(&http.body)));
    match http.status {
        408 | 409 | 429 | 500 | 502 | 503 | 504 | 529 => CallError::Retryable {
            reason,
            billed: false,
        },
        _ => CallError::Fatal { reason },
    }
}

fn send<T: Transport>(
    transport: &T,
    key: &Secret,
    url: &str,
    headers: &[(&str, String)],
    body: &Value,
) -> Result<Value, CallError> {
    let http = transport
        .post(url, headers, &body.to_string())
        .map_err(|e| CallError::Retryable {
            // A timeout may have been billed: the runner charges its worst case.
            reason: clip(&key.redact(&e)),
            billed: true,
        })?;
    if http.status != 200 {
        return Err(refused(&http, key));
    }
    let v: Value = serde_json::from_str(&http.body).map_err(|e| CallError::Retryable {
        reason: format!("HTTP 200 with a body that is not JSON: {e}"),
        billed: true,
    })?;
    Ok(json!({"parsed": v, "raw": http.body}))
}

/// The value at a JSON pointer, or null.
fn at<'a>(v: &'a Value, pointer: &str) -> &'a Value {
    v.pointer(pointer).unwrap_or(&Value::Null)
}

fn n(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}

/// OpenAI's Chat Completions.
#[derive(Debug)]
pub struct OpenAi<T> {
    pub transport: T,
    pub key: Secret,
    pub model: String,
    pub max_output_tokens: u64,
    pub seed: u64,
}

impl<T: Transport> OpenAi<T> {
    fn body(&self, r: &Request) -> Value {
        let mut b = self.parameters();
        if let Some(o) = b.as_object_mut() {
            o.insert("model".to_owned(), json!(self.model));
            o.insert(
                "messages".to_owned(),
                json!([
                    {"role": "system", "content": r.system},
                    {"role": "user", "content": r.user},
                ]),
            );
        }
        b
    }

    /// A Chat Completions response body, normalised.
    ///
    /// # Errors
    ///
    /// When the body has no choice or no usage.
    pub fn parse(body: &Value, raw: String) -> Result<Reply, String> {
        let choice = body["choices"].get(0).ok_or("no choice in the response")?;
        let u = &body["usage"];
        if !u.is_object() {
            return Err("no usage in the response".to_owned());
        }
        let input = n(&u["prompt_tokens"]);
        let cached = n(at(u, "/prompt_tokens_details/cached_tokens"));
        Ok(Reply {
            text: at(choice, "/message/content")
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            model: body["model"].as_str().unwrap_or_default().to_owned(),
            stop_reason: choice["finish_reason"].as_str().map(str::to_owned),
            usage: Usage {
                input_tokens: input,
                uncached_input_tokens: input.saturating_sub(cached),
                cached_input_tokens: cached,
                cache_write_input_tokens: 0,
                output_tokens: n(&u["completion_tokens"]),
                reasoning_tokens: at(u, "/completion_tokens_details/reasoning_tokens").as_u64(),
            },
            raw_usage: u.clone(),
            fingerprint: body["system_fingerprint"].as_str().map(str::to_owned),
            raw,
        })
    }
}

impl<T: Transport> Provider for OpenAi<T> {
    fn name(&self) -> &str {
        "openai"
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn parameters(&self) -> Value {
        json!({
            "temperature": 0,
            "max_completion_tokens": self.max_output_tokens,
            "seed": self.seed,
            "n": 1,
        })
    }

    fn max_output_tokens(&self) -> u64 {
        self.max_output_tokens
    }

    fn complete(&self, request: &Request) -> Result<Reply, CallError> {
        let headers = [
            ("content-type", "application/json".to_owned()),
            ("authorization", format!("Bearer {}", self.key.0)),
        ];
        let v = send(
            &self.transport,
            &self.key,
            "https://api.openai.com/v1/chat/completions",
            &headers,
            &self.body(request),
        )?;
        Self::parse(
            at(&v, "/parsed"),
            at(&v, "/raw").as_str().unwrap_or_default().to_owned(),
        )
        .map_err(|reason| CallError::Retryable {
            reason,
            billed: true,
        })
    }

    fn count(&self, _: &Request) -> Result<Option<u64>, CallError> {
        // `o200k_base` is this model's tokenizer and N34 pins it: the local count is exact.
        Ok(None)
    }

    fn input_bound(&self) -> (u64, u64) {
        (21, 20)
    }
}

/// Anthropic's Messages API.
#[derive(Debug)]
pub struct Anthropic<T> {
    pub transport: T,
    pub key: Secret,
    pub model: String,
    pub max_output_tokens: u64,
}

const ANTHROPIC_VERSION: &str = "2023-06-01";

impl<T: Transport> Anthropic<T> {
    fn headers(&self) -> [(&'static str, String); 3] {
        [
            ("content-type", "application/json".to_owned()),
            ("anthropic-version", ANTHROPIC_VERSION.to_owned()),
            ("x-api-key", self.key.0.clone()),
        ]
    }

    /// A Messages response body, normalised.
    ///
    /// # Errors
    ///
    /// When the body has no usage.
    pub fn parse(body: &Value, raw: String) -> Result<Reply, String> {
        let u = &body["usage"];
        if !u.is_object() {
            return Err("no usage in the response".to_owned());
        }
        let uncached = n(&u["input_tokens"]);
        let read = n(&u["cache_read_input_tokens"]);
        let write = n(&u["cache_creation_input_tokens"]);
        let text: String = body["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect()
            })
            .unwrap_or_default();
        Ok(Reply {
            text,
            model: body["model"].as_str().unwrap_or_default().to_owned(),
            stop_reason: body["stop_reason"].as_str().map(str::to_owned),
            usage: Usage {
                input_tokens: uncached + read + write,
                uncached_input_tokens: uncached,
                cached_input_tokens: read,
                cache_write_input_tokens: write,
                output_tokens: n(&u["output_tokens"]),
                reasoning_tokens: None,
            },
            raw_usage: u.clone(),
            fingerprint: None,
            raw,
        })
    }
}

impl<T: Transport> Provider for Anthropic<T> {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn parameters(&self) -> Value {
        json!({
            "temperature": 0,
            "max_tokens": self.max_output_tokens,
            "anthropic_version": ANTHROPIC_VERSION,
        })
    }

    fn max_output_tokens(&self) -> u64 {
        self.max_output_tokens
    }

    fn complete(&self, request: &Request) -> Result<Reply, CallError> {
        let body = json!({
            "model": self.model,
            "max_tokens": self.max_output_tokens,
            "temperature": 0,
            "system": request.system,
            "messages": [{"role": "user", "content": request.user}],
        });
        let v = send(
            &self.transport,
            &self.key,
            "https://api.anthropic.com/v1/messages",
            &self.headers(),
            &body,
        )?;
        Self::parse(
            at(&v, "/parsed"),
            at(&v, "/raw").as_str().unwrap_or_default().to_owned(),
        )
        .map_err(|reason| CallError::Retryable {
            reason,
            billed: true,
        })
    }

    fn count(&self, request: &Request) -> Result<Option<u64>, CallError> {
        let body = json!({
            "model": self.model,
            "system": request.system,
            "messages": [{"role": "user", "content": request.user}],
        });
        let v = send(
            &self.transport,
            &self.key,
            "https://api.anthropic.com/v1/messages/count_tokens",
            &self.headers(),
            &body,
        )?;
        Ok(at(&v, "/parsed/input_tokens").as_u64())
    }

    fn input_bound(&self) -> (u64, u64) {
        (8, 5)
    }
}

/// What the fake provider does on one call.
pub type Script = Box<dyn Fn(&Request, usize) -> Result<Reply, CallError>>;

/// A deterministic provider for tests and mutations: `script` answers each call, given the
/// request and how many calls came before it.
pub struct Fake {
    pub model: String,
    pub script: Script,
    pub calls: Cell<usize>,
    pub counts: Option<u64>,
}

impl std::fmt::Debug for Fake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fake")
            .field("model", &self.model)
            .field("calls", &self.calls.get())
            .finish_non_exhaustive()
    }
}

impl Fake {
    #[must_use]
    pub fn new(script: Script) -> Self {
        Self {
            model: "fake-1".to_owned(),
            script,
            calls: Cell::new(0),
            counts: None,
        }
    }
}

impl Provider for Fake {
    fn name(&self) -> &str {
        "fake"
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn parameters(&self) -> Value {
        json!({"temperature": 0, "max_tokens": 1024})
    }

    fn max_output_tokens(&self) -> u64 {
        1024
    }

    fn complete(&self, request: &Request) -> Result<Reply, CallError> {
        let i = self.calls.get();
        self.calls.set(i + 1);
        (self.script)(request, i)
    }

    fn count(&self, _: &Request) -> Result<Option<u64>, CallError> {
        Ok(self.counts)
    }

    fn input_bound(&self) -> (u64, u64) {
        (2, 1)
    }
}
