//! The action journal: one JSON object per line (`*-journal.jsonl`).
//!
//! Every line has `t` (UTC time), `ms` (since the journal opened), `kind`
//! and `label`; other fields depend on the kind:
//!
//! | kind | written when | extra fields |
//! |---|---|---|
//! | `session` / `exit` | start / clean exit | version, os |
//! | `checkpoint` | an Undo step is recorded (a user mutation) | — |
//! | `command_start` / `command_ok` / `command_cancel` | a feature dialog opens / ends | `dur_ms`, `feature` (definition JSON) |
//! | `undo` / `redo` | Undo / Redo | — |
//! | `status` / `error` | the status bar shows a result / an error | `text`; an error while a feature dialog is open also `command` and `attempt` (the feature it tried, JSON; [`super::replay::retry`]) |
//! | `health` | a feature becomes sick or healthy | `feature`, `text` |
//! | `state` | the part after a mutation (see below) | `seq`, `snap`, `result` |
//!
//! **Snapshots (`state`)** make a journal replayable
//! ([`super::replay`]). The part is serialised *without its body* (the body
//! is a cache of the features), and only the difference to the previous
//! snapshot is written ([`diff`]): unchanged features and sketches cost a
//! few bytes (`{"s":3}`), an edited value is a path to that value. Every
//! [`KEYFRAME`]-th snapshot is complete (`{"v":…}`), so a damaged line loses
//! at most that many steps. A snapshot larger than [`MAX_SNAPSHOT`] bytes,
//! or any state once the journal passed [`MAX_JOURNAL`] bytes, is written as
//! `snap_skipped` (replay stops there). `result` holds what regeneration gave
//! ([`Outcome`]): volume, face count and the sick features.

use std::collections::{BTreeMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Instant, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::lock;
use crate::kernel::{Real, TessellationOptions};
use crate::part::PartDocument;

/// Every this many snapshots one is complete.
pub const KEYFRAME: u64 = 25;
/// Largest snapshot written (bytes of JSON).
pub const MAX_SNAPSHOT: usize = 8 << 20;
/// Past this size the journal stops writing snapshots.
pub const MAX_JOURNAL: u64 = 256 << 20;
/// Journal lines kept in memory for crash reports.
pub const TAIL: usize = 50;

/// What a regeneration produced, compared by [`super::replay`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    /// Enclosed volume of all solids (mm³, display triangles).
    pub volume: Real,
    /// Faces of the body.
    pub faces: usize,
    /// Sick features: id → message.
    #[serde(default)]
    pub health: BTreeMap<String, String>,
    /// Built by the exact kernel.
    #[serde(default)]
    pub exact: bool,
}

impl Outcome {
    /// The outcome of `part` as it is now.
    pub fn of(part: &PartDocument) -> Self {
        let opts = TessellationOptions::default();
        let volume = part
            .body
            .solids()
            .filter_map(|(sid, _)| part.body.tessellate_solid(sid, &opts).ok())
            .map(|m| {
                m.faces
                    .triangles
                    .iter()
                    .map(|t| {
                        let [a, b, c] = t.map(|i| {
                            m.faces
                                .positions
                                .get(i as usize)
                                .map(|p| p.coords)
                                .unwrap_or_default()
                        });
                        a.dot(&b.cross(&c)) / 6.0
                    })
                    .sum::<Real>()
            })
            .sum();
        Self {
            volume,
            faces: part.body.faces().count(),
            health: part
                .feature_health
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            exact: part.exact,
        }
    }

    /// How `other` differs from this (volume to a relative 1e-6), or `None`.
    pub fn difference(&self, other: &Outcome) -> Option<String> {
        let mut out = Vec::new();
        if self.health != other.health {
            out.push(format!(
                "sick features {:?} -> {:?}",
                self.health, other.health
            ));
        }
        let tol = 1e-6 * self.volume.abs().max(other.volume.abs()).max(1.0);
        if (self.volume - other.volume).abs() > tol {
            out.push(format!("volume {} -> {}", self.volume, other.volume));
        }
        if self.faces != other.faces {
            out.push(format!("faces {} -> {}", self.faces, other.faces));
        }
        (!out.is_empty()).then(|| out.join("; "))
    }
}

// --------------------------------------------------------------- diffs ---

/// The difference turning `old` into `new`, or `None` when equal:
/// `{"v": value}` replaces, `{"o": {key: patch}, "d": [keys]}` edits an
/// object, `{"a": [element…]}` rebuilds an array where an element is
/// `{"s": j}` (the old element `j`, unchanged) or a patch of the old
/// element at the same index.
pub fn diff(old: &Value, new: &Value) -> Option<Value> {
    match (old, new) {
        (Value::Object(a), Value::Object(b)) => {
            let mut set = Map::new();
            for (k, vb) in b {
                match a.get(k) {
                    Some(va) => {
                        if let Some(p) = diff(va, vb) {
                            set.insert(k.clone(), p);
                        }
                    }
                    None => {
                        set.insert(k.clone(), json!({ "v": vb }));
                    }
                }
            }
            let del: Vec<&String> = a.keys().filter(|k| !b.contains_key(*k)).collect();
            if set.is_empty() && del.is_empty() {
                return None;
            }
            let mut p = Map::new();
            p.insert("o".into(), Value::Object(set));
            if !del.is_empty() {
                p.insert("d".into(), json!(del));
            }
            Some(Value::Object(p))
        }
        (Value::Array(a), Value::Array(b)) => {
            if a == b {
                return None;
            }
            let elems: Vec<Value> = b
                .iter()
                .enumerate()
                .map(|(i, vb)| {
                    // Same place, or shifted by an insertion / deletion.
                    let same = [Some(i), i.checked_sub(1), Some(i + 1)]
                        .into_iter()
                        .flatten()
                        .find(|&j| a.get(j) == Some(vb));
                    match (same, a.get(i)) {
                        (Some(j), _) => json!({ "s": j }),
                        (None, Some(va)) => diff(va, vb).unwrap_or_else(|| json!({ "s": i })),
                        (None, None) => json!({ "v": vb }),
                    }
                })
                .collect();
            Some(json!({ "a": elems }))
        }
        _ => (old != new).then(|| json!({ "v": new })),
    }
}

/// Applies a [`diff`] patch to `old`.
pub fn apply(old: &Value, patch: &Value) -> Result<Value, String> {
    if let Some(v) = patch.get("v") {
        return Ok(v.clone());
    }
    if let Some(set) = patch.get("o").and_then(Value::as_object) {
        let mut out = match old {
            Value::Object(m) => m.clone(),
            Value::Null => Map::new(),
            _ => return Err("object patch on a non-object".into()),
        };
        for (k, p) in set {
            let next = apply(out.get(k).unwrap_or(&Value::Null), p)?;
            out.insert(k.clone(), next);
        }
        for k in patch
            .get("d")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(k) = k.as_str() {
                out.remove(k);
            }
        }
        return Ok(Value::Object(out));
    }
    if let Some(elems) = patch.get("a").and_then(Value::as_array) {
        let empty = Vec::new();
        let a = match old {
            Value::Array(a) => a,
            Value::Null => &empty,
            _ => return Err("array patch on a non-array".into()),
        };
        return elems
            .iter()
            .enumerate()
            .map(|(i, e)| match e.get("s").and_then(Value::as_u64) {
                Some(j) => a
                    .get(j as usize)
                    .cloned()
                    .ok_or_else(|| format!("array patch refers to missing element {j}")),
                None => apply(a.get(i).unwrap_or(&Value::Null), e),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array);
    }
    Err(format!("not a patch: {patch}"))
}

/// The part as JSON without its body (a cache of the features; kept when
/// there are no features: older files store the body directly).
pub fn part_value(part: &mut PartDocument) -> Result<Value, String> {
    if part.features.is_empty() {
        return serde_json::to_value(&*part).map_err(|e| e.to_string());
    }
    let body = std::mem::take(&mut part.body);
    let v = serde_json::to_value(&*part).map_err(|e| e.to_string());
    part.body = body;
    v
}

/// A feature a command tried to add (the `attempt` of an `error` line):
/// its JSON, with `sketch` set to the profile sketch's id when the dialog
/// knows it (pass `None` for features without a profile sketch; the sketch,
/// id and name are assigned on OK, so a dialog's definition has none yet).
pub fn attempt_json(f: &crate::part::Feature, sketch: Option<&str>) -> Option<Value> {
    let mut v = serde_json::to_value(f).ok()?;
    if let (Some(id), Some(o)) = (sketch, v.as_object_mut())
        && o.get("sketch")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        o.insert("sketch".into(), json!(id));
    }
    Some(v)
}

// ------------------------------------------------------------- journal ---

/// An open journal (a file, or memory only).
#[derive(Debug)]
pub struct Journal {
    file: Option<std::io::BufWriter<std::fs::File>>,
    path: Option<PathBuf>,
    start: Instant,
    /// The last snapshot written (for the next diff).
    last: Option<Value>,
    /// Snapshots written.
    states: u64,
    bytes: u64,
    tail: VecDeque<String>,
}

impl Journal {
    /// A journal appending to `path`.
    pub fn create(path: &Path) -> Result<Self, String> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        let f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let mut j = Self::in_memory();
        j.bytes = f.metadata().map(|m| m.len()).unwrap_or(0);
        j.file = Some(std::io::BufWriter::new(f));
        j.path = Some(path.to_path_buf());
        Ok(j)
    }

    /// A journal that only keeps its last lines (no file).
    pub fn in_memory() -> Self {
        Self {
            file: None,
            path: None,
            start: Instant::now(),
            last: None,
            states: 0,
            bytes: 0,
            tail: VecDeque::with_capacity(TAIL),
        }
    }

    /// The file written to.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Writes one line of `kind` with `label` and the fields of `extra` (an
    /// object; other values are stored under `data`).
    pub fn entry(&mut self, kind: &str, label: &str, extra: Value) {
        let mut m = Map::new();
        m.insert("t".into(), json!(super::iso(SystemTime::now())));
        m.insert("ms".into(), json!(self.start.elapsed().as_millis() as u64));
        m.insert("kind".into(), json!(kind));
        m.insert("label".into(), json!(label));
        match extra {
            Value::Object(e) => m.extend(e),
            Value::Null => {}
            other => {
                m.insert("data".into(), other);
            }
        }
        let line = Value::Object(m).to_string();
        if let Some(f) = self.file.as_mut() {
            // One write + flush per line: a crash loses at most the line
            // being written.
            let ok = writeln!(f, "{line}").and_then(|()| f.flush());
            if let Err(e) = ok {
                log::warn!("journal: {e}");
                self.file = None;
            }
        }
        self.bytes += line.len() as u64 + 1;
        if self.tail.len() == TAIL {
            self.tail.pop_front();
        }
        self.tail.push_back(line);
    }

    /// Writes the part's state after the action `label` (a diff against the
    /// previous state, see the module docs). Returns `false` when nothing
    /// changed (no line is written).
    pub fn state(&mut self, label: &str, part: &mut PartDocument) -> bool {
        let value = match part_value(part) {
            Ok(v) => v,
            Err(e) => {
                self.entry("state", label, json!({ "snap_skipped": e }));
                return true;
            }
        };
        self.state_value(label, value, &Outcome::of(part))
    }

    /// [`Journal::state`] with the part already serialised.
    pub fn state_value(&mut self, label: &str, value: Value, outcome: &Outcome) -> bool {
        let keyframe = self.last.is_none() || self.states.is_multiple_of(KEYFRAME);
        let snap = if keyframe {
            Some(json!({ "v": value }))
        } else {
            match self.last.as_ref().and_then(|l| diff(l, &value)) {
                Some(p) => Some(p),
                None => return false,
            }
        };
        let seq = self.states;
        let result = serde_json::to_value(outcome).unwrap_or(Value::Null);
        let text = snap.as_ref().map(Value::to_string).unwrap_or_default();
        if self.bytes > MAX_JOURNAL || text.len() > MAX_SNAPSHOT {
            self.entry(
                "state",
                label,
                json!({ "seq": seq, "snap_skipped": format!("{} bytes", text.len()), "result": result }),
            );
            // The next state must be complete again.
            self.last = None;
        } else {
            self.entry(
                "state",
                label,
                json!({ "seq": seq, "key": keyframe, "snap": snap, "result": result }),
            );
            self.last = Some(value);
        }
        self.states += 1;
        true
    }

    /// The last lines written (oldest first).
    pub fn tail(&self) -> Vec<String> {
        self.tail.iter().cloned().collect()
    }
}

// -------------------------------------------------------------- global ---

static GLOBAL: Mutex<Option<Journal>> = Mutex::new(None);

/// Opens the session's journal (see [`super::start`]).
pub fn open_global(path: &Path) {
    match Journal::create(path) {
        Ok(j) => *lock(&GLOBAL) = Some(j),
        Err(e) => log::warn!("journal: {e}"),
    }
}

/// Runs `f` on the session's journal, if one is open.
pub fn with_global<R>(f: impl FnOnce(&mut Journal) -> R) -> Option<R> {
    lock(&GLOBAL).as_mut().map(f)
}

/// [`Journal::entry`] on the session's journal.
pub fn global_entry(kind: &str, label: &str, extra: Value) {
    with_global(|j| j.entry(kind, label, extra));
}

/// The session journal's last lines; empty when it is busy (the crash hook
/// never waits).
pub fn global_tail() -> Vec<String> {
    GLOBAL
        .try_lock()
        .ok()
        .and_then(|g| g.as_ref().map(Journal::tail))
        .unwrap_or_default()
}

/// The session journal's file.
pub fn global_path() -> Option<PathBuf> {
    lock(&GLOBAL)
        .as_ref()
        .and_then(|j| j.path().map(Path::to_path_buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_and_apply_round_trip() {
        let cases = [
            (
                json!({"a": 1, "b": [1, 2, 3]}),
                json!({"a": 2, "b": [1, 2, 3]}),
            ),
            (
                json!({"a": 1, "b": [1, 2, 3]}),
                json!({"b": [0, 1, 2, 3], "c": null}),
            ),
            (json!([{"x": 1}, {"x": 2}]), json!([{"x": 2}])),
            (
                json!([{"x": 1, "y": [5]}]),
                json!([{"x": 1, "y": [5, 6]}, 7]),
            ),
            (json!(1.5), json!("s")),
            (Value::Null, json!({"k": [1]})),
        ];
        for (a, b) in cases {
            let p = diff(&a, &b).unwrap();
            assert_eq!(apply(&a, &p).unwrap(), b, "{a} -> {b} via {p}");
            assert_eq!(diff(&b, &b), None);
        }
        // Unchanged elements cost a reference.
        let a = json!({"features": [{"big": "x".repeat(100)}, {"d": 1}]});
        let b = json!({"features": [{"big": "x".repeat(100)}, {"d": 2}]});
        let p = diff(&a, &b).unwrap();
        assert!(p.to_string().len() < 60, "{p}");
    }

    #[test]
    fn lines_parse_and_states_are_deltas() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("j.jsonl");
        let mut j = Journal::create(&path).unwrap();
        let mut part = PartDocument::with_box("Box1", [10.0, 20.0, 30.0]).unwrap();
        j.entry("checkpoint", "Box size", json!({}));
        assert!(j.state("Box size", &mut part));
        // Unchanged: nothing written.
        assert!(!j.state("Box size", &mut part));
        if let Some(b) = part.box_feature_mut() {
            b.size[0] = 11.0;
        }
        part.regenerate().unwrap();
        assert!(j.state("Box size", &mut part));
        j.entry("error", "", json!({ "text": "Extrude: no profile" }));
        drop(j);
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 4);
        for l in &lines {
            assert!(l["t"].as_str().unwrap().ends_with('Z'));
            assert!(l["kind"].is_string() && l["label"].is_string());
        }
        assert_eq!(lines[1]["key"], json!(true));
        assert_eq!(lines[2]["key"], json!(false));
        // The delta holds the new size, not the whole part.
        assert!(
            lines[2]["snap"].to_string().len() < 400,
            "{}",
            lines[2]["snap"]
        );
        assert!(lines[2]["snap"].to_string().contains("11.0"));
        // No body in the snapshots.
        assert!(!lines[1]["snap"].to_string().contains("Box1:Top"));
        let r: Outcome = serde_json::from_value(lines[2]["result"].clone()).unwrap();
        assert!((r.volume - 11.0 * 20.0 * 30.0).abs() < 1e-6, "{}", r.volume);
        assert_eq!(r.faces, 6);
    }
}
