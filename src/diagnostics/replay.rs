//! Replay of a journal: rebuilds the part state by state (the `state`
//! lines, see [`super::journal`]), regenerates after each one and compares
//! with what the session recorded. The first step whose result differs is
//! where "something stopped working" — and a ready test case:
//!
//! ```no_run
//! let steps = cadcraft3d::diagnostics::replay::replay("session-journal.jsonl".as_ref()).unwrap();
//! if let Some(s) = cadcraft3d::diagnostics::replay::first_difference(&steps) {
//!     panic!("step {} ({}): {:?}", s.seq, s.label, s.difference);
//! }
//! ```

use std::path::Path;

use serde_json::Value;

use super::journal::{Outcome, apply};
use crate::part::{BuildCache, PartDocument};

/// One replayed state.
#[derive(Clone, Debug)]
pub struct Step {
    /// Line in the journal (1-based).
    pub line: usize,
    /// The state's number in the session.
    pub seq: u64,
    /// The action that led to it.
    pub label: String,
    /// The time recorded.
    pub time: String,
    /// What the session's regeneration gave.
    pub recorded: Option<Outcome>,
    /// What the replay's regeneration gave.
    pub replayed: Option<Outcome>,
    /// The step could not be replayed (damaged line, bad snapshot, failed
    /// regeneration).
    pub error: Option<String>,
    /// How the replay differs from the recording.
    pub difference: Option<String>,
}

impl Step {
    /// The replay did not reproduce the recording.
    pub fn differs(&self) -> bool {
        self.error.is_some() || self.difference.is_some()
    }
}

/// Replays the journal at `path`.
pub fn replay(path: &Path) -> Result<Vec<Step>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    replay_text(&text)
}

/// Replays journal text (see [`replay`]).
pub fn replay_text(text: &str) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    walk(text, |step, _| {
        steps.push(step);
        true
    })?;
    Ok(steps)
}

/// The part as rebuilt at state `seq` (regenerated).
pub fn part_at(path: &Path, seq: u64) -> Result<PartDocument, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut found = None;
    walk(&text, |step, part| {
        if step.seq == seq {
            found = Some(part.clone());
            return false;
        }
        true
    })?;
    found.ok_or_else(|| format!("no state {seq} in the journal"))
}

/// The first step that differs from its recording.
pub fn first_difference(steps: &[Step]) -> Option<&Step> {
    steps.iter().find(|s| s.differs())
}

/// Walks the `state` lines; `visit` gets each step and the part after it,
/// and returns whether to go on.
fn walk(text: &str, mut visit: impl FnMut(Step, &PartDocument) -> bool) -> Result<(), String> {
    let mut current: Option<Value> = None;
    let mut part = PartDocument::default();
    let mut cache = BuildCache::default();
    let lines: Vec<&str> = text.lines().collect();
    let mut any = false;
    for (i, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            // The last line of a crashed session may be cut off.
            Err(_) if i + 1 == lines.len() => break,
            Err(e) => {
                any = true;
                let step = Step {
                    line: i + 1,
                    seq: u64::MAX,
                    label: String::new(),
                    time: String::new(),
                    recorded: None,
                    replayed: None,
                    error: Some(format!("damaged line: {e}")),
                    difference: None,
                };
                if !visit(step, &part) {
                    return Ok(());
                }
                continue;
            }
        };
        if v.get("kind").and_then(Value::as_str) != Some("state") {
            continue;
        }
        any = true;
        let mut step = Step {
            line: i + 1,
            seq: v.get("seq").and_then(Value::as_u64).unwrap_or(u64::MAX),
            label: str_of(&v, "label"),
            time: str_of(&v, "t"),
            recorded: v
                .get("result")
                .and_then(|r| serde_json::from_value(r.clone()).ok()),
            replayed: None,
            error: None,
            difference: None,
        };
        match rebuild(&v, &mut current, &mut cache) {
            Ok(p) => {
                let got = Outcome::of(&p);
                step.difference = step.recorded.as_ref().and_then(|r| r.difference(&got));
                step.replayed = Some(got);
                part = p;
            }
            Err(e) => step.error = Some(e),
        }
        if !visit(step, &part) {
            return Ok(());
        }
    }
    if !any {
        return Err("the journal has no recorded states".into());
    }
    Ok(())
}

/// A feature command whose OK (or a field) failed: an `error` line with the
/// feature it tried (`attempt`).
#[derive(Clone, Debug)]
pub struct Attempt {
    /// Line in the journal (1-based).
    pub line: usize,
    /// The last state recorded before it: [`part_at`] of it is the part the
    /// command saw.
    pub state: Option<u64>,
    /// The command (Inventor's name).
    pub command: String,
    /// The error the user saw.
    pub text: String,
    /// The feature (JSON, as [`crate::part::Feature`] serialises).
    pub feature: Value,
}

/// Every failed attempt in journal text, in order.
pub fn attempts(text: &str) -> Vec<Attempt> {
    let mut state = None;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match v.get("kind").and_then(Value::as_str) {
            Some("state") => state = v.get("seq").and_then(Value::as_u64).or(state),
            Some("error") => {
                if let Some(f) = v.get("attempt") {
                    out.push(Attempt {
                        line: i + 1,
                        state,
                        command: str_of(&v, "command"),
                        text: str_of(&v, "text"),
                        feature: f.clone(),
                    });
                }
            }
            _ => {}
        }
    }
    out
}

/// Adds an attempted feature to `part` as the command's OK does (a new id
/// and name when it has none); the feature's error when it fails, with
/// `part` unchanged.
pub fn retry(part: &mut PartDocument, feature: &Value) -> Result<String, String> {
    let mut v = feature.clone();
    let kind = str_of(&v, "type");
    if str_of(&v, "id").is_empty()
        && let Some(o) = v.as_object_mut()
    {
        let id = part.next_feature_id(&kind);
        o.insert("id".into(), Value::String(id.clone()));
        o.insert("name".into(), Value::String(id));
    }
    let f: crate::part::Feature =
        serde_json::from_value(v).map_err(|e| format!("the attempt is not a feature: {e}"))?;
    let id = f.id().to_owned();
    part.insert_feature_checked(f).map_err(|e| e.to_string())?;
    Ok(id)
}

fn str_of(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Applies a state line to `current` and regenerates the part.
fn rebuild(
    v: &Value,
    current: &mut Option<Value>,
    cache: &mut BuildCache,
) -> Result<PartDocument, String> {
    if let Some(why) = v.get("snap_skipped") {
        *current = None;
        return Err(format!("the session did not record this state ({why})"));
    }
    let snap = v.get("snap").ok_or("the state has no snapshot")?;
    let base = match (current.as_ref(), snap.get("v").is_some()) {
        (Some(c), _) => c.clone(),
        (None, true) => Value::Null,
        (None, false) => return Err("a delta without the state before it".into()),
    };
    let next = match apply(&base, snap) {
        Ok(n) => n,
        Err(e) => {
            // The following deltas need a complete state again.
            *current = None;
            return Err(e);
        }
    };
    let mut part: PartDocument =
        serde_json::from_value(next.clone()).map_err(|e| format!("snapshot: {e}"))?;
    *current = Some(next);
    // Steps share the build cache (as the session did): only what changed
    // is rebuilt.
    part.build_cache = cache.clone();
    part.regenerate()
        .map_err(|e| format!("regeneration: {e}"))?;
    *cache = part.build_cache.clone();
    Ok(part)
}
