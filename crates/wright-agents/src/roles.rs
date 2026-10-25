//! Per-role model + effort: one table, overridable by env or flag.
//!
//! Precedent: Aider's `ModelSettings` (Aider-AI/aider `aider/models.py`):
//! a main model plus `weak_model_name` / `editor_model_name` per role, where
//! `Model.get_weak_model` / `get_editor_model` fall back to the main model
//! when the role's name is unset. Here the same fallback holds for the
//! rebuttal proposer ([`Models::proposer`] `None` = the specialist's model,
//! i.e. the model that did the designing argues for it).
//!
//! Defaults (10/24 night, measured in docs/TODO.md "Cost + latency"):
//!
//! | role | model | effort |
//! |---|---|---|
//! | orchestrator | Opus 5.5 (Fable 5.1 with `--demo` / `WRIGHT_DEMO=1`) | medium |
//! | specialist | Opus 5.5 | medium |
//! | reviewer (and rebuttal adjudicator) | Opus 5.5 | high |
//! | proposer (rebuttal) | = specialist | = specialist |
//! | elicit | Sonnet 5.5 | low |
//!
//! Overrides: `WRIGHT_MODEL_<ROLE>=model[:effort[:max_tokens]]` (role in
//! upper case: ORCHESTRATOR, SPECIALIST, REVIEWER, PROPOSER, ELICIT), or
//! the CLI's `--model role=model[:effort[:max_tokens]]`, which wins over env.

use crate::agent::ModelCfg;
use crate::harness::Models;

pub const ROLES: &[&str] = &["orchestrator", "specialist", "reviewer", "proposer", "elicit"];

/// Is the demo switch on (`WRIGHT_DEMO=1`)? The CLI also takes `--demo`.
pub fn demo_from_env() -> bool {
    std::env::var("WRIGHT_DEMO").map(|v| !v.is_empty() && v != "0").unwrap_or(false)
}

/// Parse `model[:effort[:max_tokens]]` over a base config (unset parts keep the base).
pub fn parse_cfg(text: &str, base: &ModelCfg) -> Result<ModelCfg, String> {
    let mut parts = text.trim().split(':');
    let id = parts.next().filter(|s| !s.is_empty()).ok_or_else(|| format!("`{text}`: no model id"))?;
    let mut c = ModelCfg::new(id, &base.effort, base.max_tokens);
    if let Some(e) = parts.next().filter(|s| !s.is_empty()) {
        if !["low", "medium", "high", "xhigh", "max"].contains(&e) {
            return Err(format!("`{text}`: effort must be low|medium|high|xhigh|max"));
        }
        c.effort = e.into();
    }
    if let Some(m) = parts.next().filter(|s| !s.is_empty()) {
        c.max_tokens = m.parse().map_err(|_| format!("`{text}`: max_tokens `{m}` is not a number"))?;
    }
    Ok(c)
}

impl Models {
    /// Set one role from `model[:effort[:max_tokens]]`.
    pub fn set(&mut self, role: &str, text: &str) -> Result<(), String> {
        match role {
            "orchestrator" => self.orchestrator = parse_cfg(text, &self.orchestrator)?,
            "specialist" => self.specialist = parse_cfg(text, &self.specialist)?,
            "reviewer" => self.reviewer = parse_cfg(text, &self.reviewer)?,
            "proposer" => self.proposer = Some(parse_cfg(text, &self.proposer())?),
            "elicit" => self.elicit = parse_cfg(text, &self.elicit)?,
            other => return Err(format!("no role `{other}`; roles are {}", ROLES.join(", "))),
        }
        Ok(())
    }

    /// The standard table with `WRIGHT_MODEL_<ROLE>` overrides applied
    /// (errors name the variable; a bad override is refused, not ignored).
    pub fn from_env(demo: bool) -> Result<Self, String> {
        let mut m = Models::standard(demo);
        for role in ROLES {
            let var = format!("WRIGHT_MODEL_{}", role.to_uppercase());
            if let Ok(v) = std::env::var(&var) {
                if !v.trim().is_empty() {
                    m.set(role, &v).map_err(|e| format!("{var}: {e}"))?;
                }
            }
        }
        Ok(m)
    }

    /// The rebuttal proposer: its own override, else the specialist's model.
    pub fn proposer(&self) -> ModelCfg {
        self.proposer.clone().unwrap_or_else(|| self.specialist.clone())
    }

    /// `(role, model, effort, max_tokens)` for every role, in [`ROLES`] order.
    pub fn mapping(&self) -> Vec<(&'static str, ModelCfg)> {
        vec![
            ("orchestrator", self.orchestrator.clone()),
            ("specialist", self.specialist.clone()),
            ("reviewer", self.reviewer.clone()),
            ("proposer", self.proposer()),
            ("elicit", self.elicit.clone()),
        ]
    }

    /// One line for logs: `orchestrator=claude-opus-5-5/medium, ...`.
    pub fn describe(&self) -> String {
        self.mapping().iter().map(|(r, c)| format!("{r}={}/{}", c.id, c.effort)).collect::<Vec<_>>().join(", ")
    }
}

/// Record the role -> model mapping as one INTERNAL span `wright.models`
/// in `<run>/trace.jsonl` (attributes `wright.model.<role>` =
/// `model/effort/max_tokens`), so a trace says which model each role ran on.
pub fn trace_mapping(tracer: &crate::trace::Tracer, models: &Models) {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mut attributes = std::collections::BTreeMap::new();
    attributes.insert("gen_ai.operation.name".to_string(), serde_json::json!("wright.models"));
    for (role, c) in models.mapping() {
        attributes.insert(format!("wright.model.{role}"), serde_json::json!(format!("{}/{}/{}", c.id, c.effort, c.max_tokens)));
    }
    tracer.record(crate::trace::Span {
        trace_id: tracer.trace_id.clone(),
        span_id: tracer.new_span_id(),
        parent_span_id: None,
        name: "wright.models".into(),
        kind: crate::trace::KIND_INTERNAL,
        start_unix_nanos: now,
        end_unix_nanos: now,
        attributes,
        status: crate::trace::STATUS_OK,
        status_message: String::new(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_the_table_and_demo_puts_fable_on_the_orchestrator() {
        let m = Models::standard(false);
        assert_eq!(m.orchestrator.id, "claude-opus-5-5");
        assert_eq!(m.orchestrator.effort, "medium");
        assert_eq!((m.reviewer.id.as_str(), m.reviewer.effort.as_str()), ("claude-opus-5-5", "high"));
        assert_eq!((m.elicit.id.as_str(), m.elicit.effort.as_str()), ("claude-sonnet-5-5", "low"));
        assert_eq!(m.proposer(), m.specialist);
        assert_eq!(Models::standard(true).orchestrator.id, "claude-fable-5-1");
    }

    #[test]
    fn overrides_parse_and_bad_ones_are_refused() {
        let mut m = Models::standard(false);
        m.set("orchestrator", "claude-fable-5-1:low").unwrap();
        assert_eq!((m.orchestrator.id.as_str(), m.orchestrator.effort.as_str(), m.orchestrator.max_tokens), ("claude-fable-5-1", "low", 8000));
        m.set("proposer", "claude-sonnet-5-5::2000").unwrap();
        assert_eq!(m.proposer().max_tokens, 2000);
        assert_eq!(m.proposer().effort, "medium");
        assert!(m.set("orchestrator", "x:turbo").is_err());
        assert!(m.set("nobody", "x").is_err());
        assert!(m.describe().starts_with("orchestrator=claude-fable-5-1/low, specialist=claude-opus-5-5/medium"));
    }
}
