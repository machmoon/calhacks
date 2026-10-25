//! A scripted [`Transport`] for offline tests and demos: no network, no key.
//! Responses are played back in order; every request is recorded so a test
//! can assert on exactly what the model would have been sent.

use crate::client::{ApiError, BoxFut, Provider, Request, Transport};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::Mutex;

type Responder = Box<dyn FnMut(&Value) -> Option<Value> + Send>;

pub struct ScriptedTransport {
    script: Mutex<Responder>,
    pub requests: Mutex<Vec<Request>>,
}

impl ScriptedTransport {
    /// Play `responses` back in order; one more call is an error.
    pub fn new(responses: Vec<Value>) -> Self {
        let mut q: VecDeque<Value> = responses.into();
        Self::from_fn(move |_| q.pop_front())
    }
    /// Answer each request body with `f`; `None` ends the script.
    pub fn from_fn(f: impl FnMut(&Value) -> Option<Value> + Send + 'static) -> Self {
        ScriptedTransport { script: Mutex::new(Box::new(f)), requests: Mutex::new(vec![]) }
    }
    pub fn calls(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

impl Transport for ScriptedTransport {
    fn provider(&self) -> Provider {
        Provider::Fake
    }
    fn send<'a>(&'a self, req: &'a Request) -> BoxFut<'a, Result<Value, ApiError>> {
        Box::pin(async move {
            let n = {
                let mut r = self.requests.lock().unwrap();
                r.push(req.clone());
                r.len()
            };
            (self.script.lock().unwrap())(&req.body).ok_or(ApiError::ScriptExhausted(n - 1))
        })
    }
}

/// A Messages API response with the given blocks. `stop_reason` follows
/// from the content: `tool_use` if any block is a tool call.
pub fn reply(model: &str, content: Vec<Value>, input_tokens: u64, output_tokens: u64) -> Value {
    let stop = if content.iter().any(|b| b["type"] == "tool_use") { "tool_use" } else { "end_turn" };
    json!({
        "id": "msg_fake", "type": "message", "role": "assistant", "model": model,
        "content": content, "stop_reason": stop,
        "usage": {"input_tokens": input_tokens, "output_tokens": output_tokens,
                  "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}
    })
}

pub fn text(t: &str) -> Value {
    json!({"type": "text", "text": t})
}

pub fn tool_use(id: &str, name: &str, input: Value) -> Value {
    json!({"type": "tool_use", "id": id, "name": name, "input": input})
}
