//! One agent's tool loop over the Messages API.
//!
//! The loop is the documented manual loop (`docs/research/harness.md`,
//! "Tool loop"): append the assistant's whole `content` array, answer every
//! `tool_use` in one user message of `tool_result` blocks (`is_error: true`
//! for a failed tool or check, with the numbers), repeat. History is
//! append-only -- nothing sent is ever edited -- because preserved thinking
//! rejects edited histories and any edit also invalidates the prompt cache.
//!
//! Malformed actions (an unknown tool, input that does not parse, output cut
//! off at `max_tokens`) are answered with an error and requeried, at most
//! [`MAX_REQUERIES`] times in a row before the agent fails: SWE-agent's
//! `forward_with_handling` (SWE-agent @3ea751c, `sweagent/agent/agents.py`)
//! with its default of 3.
//!
//! A tool can end the loop (`submit`, `report`, `verdict`). Its
//! `tool_result` is then owed to the model; it is kept in
//! [`Conversation::pending`] and leads the next user message, so a
//! conversation can resume next round without an unanswered `tool_use`.

use crate::client::{BoxFut, CallError, Client};
use serde_json::{json, Value};
use wright_core::Event;

pub const MAX_REQUERIES: u32 = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct ModelCfg {
    pub id: String,
    /// `output_config.effort`: low | medium | high | xhigh | max.
    pub effort: String,
    pub max_tokens: u32,
}

impl ModelCfg {
    pub fn new(id: &str, effort: &str, max_tokens: u32) -> Self {
        ModelCfg { id: id.into(), effort: effort.into(), max_tokens }
    }
}

pub struct Conversation {
    /// The name events carry: "orchestrator", "specialist:aero", "reviewer".
    pub agent: String,
    pub model: ModelCfg,
    /// Fixed per (role, domain): no run ids or timestamps, so it caches.
    pub system: String,
    /// Fixed order, fixed content: part of the cached prefix.
    pub tools: Vec<Value>,
    pub messages: Vec<Value>,
    pub pending: Vec<Value>,
}

impl Conversation {
    pub fn new(agent: impl Into<String>, model: ModelCfg, system: String, tools: Vec<Value>) -> Self {
        Conversation { agent: agent.into(), model, system, tools, messages: vec![], pending: vec![] }
    }

    /// The request body. Caching per `docs/research/harness.md`: one
    /// explicit breakpoint on the last block of the fixed system prompt
    /// (tools render before system, so it covers both), plus top-level
    /// automatic caching for the growing conversation
    /// (`shared/prompt-caching.md`, "The robust combination for agent
    /// loops"). `tool_choice` is `auto`: forced choice is a 400 on Sonnet
    /// and Opus 5.5. No `thinking` field: adaptive is the default and the
    /// only control is effort. No sampling parameters (a 400 since
    /// 2026-10-21).
    pub fn body(&self) -> Value {
        json!({
            "model": self.model.id,
            "max_tokens": self.model.max_tokens,
            "system": [{"type": "text", "text": self.system, "cache_control": {"type": "ephemeral"}}],
            "tools": self.tools,
            "tool_choice": {"type": "auto"},
            "messages": self.messages,
            "output_config": {"effort": self.model.effort},
            "cache_control": {"type": "ephemeral"},
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct ToolOutput {
    pub content: String,
    /// Optional multimodal result blocks; text-only callers retain the original string shape.
    pub blocks: Vec<Value>,
    pub is_error: bool,
    /// The call itself was malformed (counts toward the requery limit).
    pub malformed: bool,
    /// End this agent's loop after answering.
    pub stop: bool,
}

impl ToolOutput {
    pub fn ok(s: impl Into<String>) -> Self {
        ToolOutput { content: s.into(), ..Default::default() }
    }
    pub fn err(s: impl Into<String>) -> Self {
        ToolOutput { content: s.into(), is_error: true, ..Default::default() }
    }
    pub fn malformed(s: impl Into<String>) -> Self {
        ToolOutput { content: s.into(), is_error: true, malformed: true, stop: false, blocks: vec![] }
    }
    pub fn with_blocks(mut self, blocks: Vec<Value>) -> Self { self.blocks = blocks; self }
    pub fn stop(mut self) -> Self {
        self.stop = true;
        self
    }
}

pub trait Handler: Send {
    fn call<'a>(&'a mut self, name: &'a str, input: &'a Value) -> BoxFut<'a, ToolOutput>;
    /// Observe just this turn, without copying the entire conversation history.
    fn observe_user_blocks(&mut self, _blocks: &[Value]) {}
    fn observe_tool_result(&mut self, _name: &str, _input: &Value, _output: &ToolOutput) {}
    /// Blocks to append to the next user message (a person's feedback).
    fn extra_blocks(&mut self) -> Vec<Value> {
        vec![]
    }
    /// Answer a whole turn's `(name, input)` calls at once (`Some`, one
    /// output per call in order), or `None` to have them run one by one
    /// through [`Handler::call`]. The harness fans independent `delegate`
    /// calls out concurrently here (`crate::merge`).
    fn call_many<'a>(&'a mut self, _calls: &'a [(String, Value)]) -> BoxFut<'a, Option<Vec<ToolOutput>>> {
        Box::pin(async { None })
    }
    /// After a turn's tool calls are answered and before the next model
    /// call: `owed` are the `tool_result` blocks not yet sent, `turns` the
    /// model calls made so far in this `run_agent`. The harness checkpoints
    /// here (`crate::checkpoint`).
    fn after_turn(&mut self, _conv: &Conversation, _owed: &[Value], _turns: u32) {}
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentEnd {
    /// The model ended its turn without a tool call.
    Done { text: String },
    /// A tool asked to stop.
    Stopped { text: String },
    TurnLimit,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error(transparent)]
    Call(#[from] CallError),
    #[error("malformed tool input {0} times in a row: {1}")]
    Malformed(u32, String),
    #[error("the model refused: {0}")]
    Refusal(String),
}

fn texts(content: &[Value]) -> String {
    content
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Run `conv` from a new user turn `task` until it ends, a tool stops it,
/// or `max_turns` model calls have been made.
pub async fn run_agent<H: Handler>(
    client: &Client,
    conv: &mut Conversation,
    task: Vec<Value>,
    handler: &mut H,
    max_turns: u32,
) -> Result<AgentEnd, AgentError> {
    let mut user: Vec<Value> = std::mem::take(&mut conv.pending);
    user.extend(task);
    user.extend(handler.extra_blocks());
    let mut strikes = 0u32;
    let mut last_text = String::new();

    for turn in 0..max_turns {
        handler.observe_user_blocks(&user);
        conv.messages.push(json!({"role": "user", "content": std::mem::take(&mut user)}));
        let resp = client.create(conv.body()).await?;
        // A reply with no content array or an unknown stop reason is not an
        // answer (an error object, a truncated proxy body): never read it as
        // "done", which a reviewer's silence would turn into an approval.
        let stop_reason = resp["stop_reason"].as_str().unwrap_or("");
        let known = ["end_turn", "tool_use", "max_tokens", "stop_sequence", "pause_turn", "refusal"];
        let Some(content) = resp["content"].as_array().cloned().filter(|_| known.contains(&stop_reason)) else {
            // resend the same user turn
            if let Some(m) = conv.messages.pop() {
                user = m["content"].as_array().cloned().unwrap_or_default();
            }
            strikes += 1;
            if strikes > MAX_REQUERIES {
                return Err(AgentError::Malformed(strikes, format!("unusable reply: {}", short(&resp))));
            }
            continue;
        };
        conv.messages.push(json!({"role": "assistant", "content": content}));

        let text = texts(&content);
        if !text.is_empty() {
            last_text = text.clone();
            let _ = client.events.send(Event::AgentMessage { run: client.run.clone(), agent: conv.agent.clone(), text });
        }
        if stop_reason == "refusal" {
            return Err(AgentError::Refusal(resp["stop_details"].to_string()));
        }

        let calls: Vec<&Value> = content.iter().filter(|b| b["type"] == "tool_use").collect();
        if calls.is_empty() {
            if stop_reason == "max_tokens" {
                strikes += 1;
                if strikes > MAX_REQUERIES {
                    return Err(AgentError::Malformed(strikes, "cut off at max_tokens".into()));
                }
                user.push(json!({"type": "text", "text": "Your reply was cut off at max_tokens. Reply shorter, and act through one tool call."}));
                user.extend(handler.extra_blocks());
                continue;
            }
            return Ok(AgentEnd::Done { text: last_text });
        }

        // several calls in one turn may be answered together (parallel specialists)
        let batched = if calls.len() > 1 && stop_reason != "max_tokens" {
            let batch: Vec<(String, Value)> = calls.iter().map(|c| (c["name"].as_str().unwrap_or("").to_string(), c["input"].clone())).collect();
            let out = handler.call_many(&batch).await;
            // the batch ran together: announce every call up front (the
            // per-call loop below announces only when the batch declined)
            if out.is_some() {
                for call in &calls {
                    let _ = client.events.send(Event::ToolCall { run: client.run.clone(), agent: conv.agent.clone(), tool: call["name"].as_str().unwrap_or("").into(), input: call["input"].clone() });
                }
            }
            out
        } else {
            None
        };
        let mut bad = None;
        let mut stop = false;
        for (i, call) in calls.into_iter().enumerate() {
            let name = call["name"].as_str().unwrap_or("");
            let id = call["id"].as_str().unwrap_or("");
            let input = &call["input"];
            if batched.is_none() {
                let _ = client.events.send(Event::ToolCall {
                    run: client.run.clone(),
                    agent: conv.agent.clone(),
                    tool: name.into(),
                    input: input.clone(),
                });
            }
            let out = if stop_reason == "max_tokens" {
                ToolOutput::malformed("this call was cut off at max_tokens and its input may be incomplete; resend it shorter")
            } else if let Some(b) = &batched {
                b.get(i).cloned().unwrap_or_else(|| ToolOutput::err("no result for this call in the batch"))
            } else {
                handler.call(name, input).await
            };
            handler.observe_tool_result(name, input, &out);
            let _ = client.events.send(Event::ToolResult {
                run: client.run.clone(),
                agent: conv.agent.clone(),
                tool: name.into(),
                ok: !out.is_error,
                summary: out.content.chars().take(400).collect(),
            });
            if out.malformed {
                bad = Some(format!("{name}: {}", out.content));
            }
            stop |= out.stop;
            let result_content = if out.blocks.is_empty() { json!(out.content) } else {
                let mut blocks=vec![json!({"type":"text","text":out.content})];blocks.extend(out.blocks);json!(blocks)
            };
            user.push(json!({"type": "tool_result", "tool_use_id": id, "content": result_content, "is_error": out.is_error}));
        }
        match bad {
            Some(why) => {
                strikes += 1;
                if strikes > MAX_REQUERIES {
                    conv.pending = user;
                    return Err(AgentError::Malformed(strikes, why));
                }
            }
            None => strikes = 0,
        }
        handler.after_turn(conv, &user, turn + 1);
        if stop {
            conv.pending = user;
            return Ok(AgentEnd::Stopped { text: last_text });
        }
        user.extend(handler.extra_blocks());
    }
    conv.pending = user;
    Ok(AgentEnd::TurnLimit)
}

/// Write a conversation's full history to `dir/<name>.json`: `agent`,
/// `model`, `system`, `tools`, `messages` exactly as sent (append-only, so
/// the last request plus the final reply) and any `pending` tool results
/// still owed to the model. This is what `wright-train` turns into
/// post-training data; `events.jsonl` only summarises it (tool results are
/// cut to 400 characters there). SWE-agent keeps the same record in its
/// `.traj` files, and SWE-smith's exporter reads the full message history
/// out of the last step (`swesmith/train/traj_mgr/utils.py`,
/// `get_messages`).
pub fn save_transcript(dir: &std::path::Path, name: &str, conv: &Conversation) -> std::io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{name}.json"));
    let v = json!({
        "agent": conv.agent,
        "model": {"id": conv.model.id, "effort": conv.model.effort, "max_tokens": conv.model.max_tokens},
        "system": conv.system,
        "tools": conv.tools,
        "messages": conv.messages,
        "pending": conv.pending,
    });
    std::fs::write(&path, serde_json::to_vec(&v)?)?;
    Ok(path)
}

fn short(v: &Value) -> String {
    let t = v.to_string();
    if t.len() > 200 { format!("{}...", &t[..t.char_indices().nth(200).map_or(t.len(), |(i, _)| i)]) } else { t }
}

#[cfg(test)]
mod multimodal_result_tests {
    use super::*;
    use std::sync::Arc;
    #[tokio::test]
    async fn image_results_are_blocks_and_legacy_text_stays_string() {
        struct Output(ToolOutput);
        impl Handler for Output {
            fn call<'a>(&'a mut self,_:&'a str,_:&'a Value)->BoxFut<'a,ToolOutput> {Box::pin(async move {self.0.clone()})}
        }
        for multimodal in [false,true] {
            let transport=Arc::new(crate::fake::ScriptedTransport::new(vec![crate::fake::reply("m",vec![crate::fake::tool_use("id","render",json!({}))],1,1),crate::fake::reply("m",vec![crate::fake::text("done")],1,1)]));
            let (events,_)=crate::channel();let client=Client::new(transport.clone(),crate::Budget::new(5.,45.,None),events,"images");
            let mut conv=Conversation::new("test",ModelCfg::new("m","low",100),String::new(),vec![]);
            let mut output=ToolOutput::ok("native render result");
            if multimodal{output=output.with_blocks(vec![json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"test"}})]);}
            run_agent(&client,&mut conv,vec![],&mut Output(output),2).await.unwrap();
            let requests=transport.requests.lock().unwrap();let content=&requests[1].body["messages"][2]["content"][0]["content"];
            if multimodal{assert_eq!(content[0]["text"],"native render result");assert_eq!(content[1]["type"],"image");}
            else{assert_eq!(content,"native render result");}
        }
    }
}
