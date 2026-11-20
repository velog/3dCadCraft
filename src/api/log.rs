//! The AI action log (spec 0.7 §3.6a): one line `kind: "ai"` per tool call in the session's
//! action journal ([`crate::diagnostics::journal`]), so Help › Save Diagnostics shows what an agent
//! did next to what the user did.

use serde_json::{Value, json};

use super::session::ApiError;
use super::tools::{Effect, Tool};

/// Longest answer kept in a line (characters); longer ones are cut and marked.
pub const MAX_ANSWER: usize = 2000;

/// Where the call came from: the headless server or the running app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    /// `3dcadcraft-mcp`, `3dcadcraft --mcp`.
    Headless,
    /// The app, through `--attach` (spec 0.7 §3.9).
    App,
}

/// The journal line of a call (without the time fields the journal adds).
pub fn line(
    via: Via,
    tool: &Tool,
    args: &Value,
    result: &Result<Value, ApiError>,
    ms: u128,
) -> Value {
    let mut v = json!({
        "source": "ai",
        "via": match via { Via::Headless => "headless", Via::App => "app" },
        "tool": tool.name,
        "arguments": args,
        "ok": result.is_ok(),
        "work_ms": ms as u64,
    });
    match result {
        Err(e) => v["error"] = json!(e.to_string()),
        // Queries answer long lists the agent already has: only changes keep their answer.
        Ok(a) if tool.effect != Effect::Read => {
            let text = a.to_string();
            v["answer"] = if text.chars().count() > MAX_ANSWER {
                let cut: String = text.chars().take(MAX_ANSWER).collect();
                json!(format!("{cut}... (cut)"))
            } else {
                a.clone()
            };
        }
        Ok(_) => {}
    }
    v
}

/// Writes the line of a call into the session's journal (nothing when none is open, as in
/// tests). `status` is polled while work runs and is not logged.
pub fn record(via: Via, tool: &Tool, args: &Value, result: &Result<Value, ApiError>, ms: u128) {
    if tool.name == "status" {
        return;
    }
    crate::diagnostics::journal::global_entry(
        "ai",
        tool.command,
        line(via, tool, args, result, ms),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_name_the_source_and_cut_long_answers() {
        let t = super::super::tools::find("extrude").unwrap_or_else(|| panic!("no extrude"));
        let ok: Result<Value, ApiError> = Ok(json!({"feature": "Extrusion1"}));
        let v = line(Via::Headless, t, &json!({"distance": 5}), &ok, 12);
        assert_eq!(v["source"], "ai");
        assert_eq!(v["tool"], "extrude");
        assert_eq!(v["answer"]["feature"], "Extrusion1");
        let long: Result<Value, ApiError> = Ok(json!({"x": "a".repeat(5000)}));
        let v = line(Via::App, t, &json!({}), &long, 0);
        assert!(v["answer"].as_str().unwrap_or_default().ends_with("(cut)"));
        assert_eq!(v["via"], "app");
        let q = super::super::tools::find("faces").unwrap_or_else(|| panic!("no faces"));
        let v = line(Via::Headless, q, &json!({}), &Ok(json!({"faces": []})), 0);
        assert!(v.get("answer").is_none());
        let e: Result<Value, ApiError> = Err(ApiError::new("Extrude", "no sketch 'X'"));
        let v = line(Via::Headless, t, &json!({}), &e, 0);
        assert_eq!(v["ok"], false);
        assert_eq!(v["error"], "Extrude: no sketch 'X'");
    }
}
