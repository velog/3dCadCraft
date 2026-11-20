//! The state an agent works on: one project, its file, the active document and an Undo history.
//!
//! A [`Session`] is the command layer's whole world. The MCP server owns one; the GUI will own
//! one for the agent connected to it (spec 0.7 §6), so every tool is a plain function of
//! `(&mut Session, arguments)` with no transport in it.

use std::path::PathBuf;

use crate::command::History;
use crate::document::{AssemblyDocument, DocumentData, DocumentId, Project};
use crate::part::PartDocument;

/// A tool's failure, shown to the agent as `<Command>: <reason>` (spec 0-ui error wording).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiError {
    /// The command's name as the user knows it (`Extrude`, `Hole`).
    pub command: String,
    /// What went wrong, in one sentence.
    pub reason: String,
}

impl ApiError {
    /// `<command>: <reason>`.
    pub fn new(command: &str, reason: impl Into<String>) -> Self {
        Self {
            command: command.to_owned(),
            reason: reason.into(),
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.command, self.reason)
    }
}

impl std::error::Error for ApiError {}

/// What Undo restores: the project and which document was active.
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// The project as it was.
    pub project: Project,
    /// The active document then.
    pub active: Option<DocumentId>,
}

/// One project being worked on by an agent.
#[derive(Debug)]
pub struct Session {
    /// The project.
    pub project: Project,
    /// Where it was opened from / last saved to.
    pub path: Option<PathBuf>,
    /// The document part and assembly tools work on.
    pub active: Option<DocumentId>,
    /// Undo / Redo of whole tool calls.
    pub history: History<Snapshot>,
    /// The tools changed the project since it was opened or saved.
    pub dirty: bool,
    /// Only reading tools run (`--read-only`, spec 0.7 §3.6).
    pub read_only: bool,
    /// The session is the running app's (spec 0.7 §3.9): the project tools that would replace
    /// the user's project are refused.
    pub hosted: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self::new(Project::new("Untitled"))
    }
}

impl Clone for Session {
    /// A copy for a worker: the project and its place, without the Undo history (the worker's
    /// result is taken back into the session it came from, whose history records it).
    fn clone(&self) -> Self {
        Self {
            project: self.project.clone(),
            path: self.path.clone(),
            active: self.active,
            history: History::new(1),
            dirty: self.dirty,
            read_only: self.read_only,
            hosted: self.hosted,
        }
    }
}

impl Session {
    /// A session on `project`, its first document active.
    pub fn new(project: Project) -> Self {
        let active = project.documents().first().map(|d| d.id);
        Self {
            project,
            path: None,
            active,
            history: History::new(100),
            dirty: false,
            read_only: false,
            hosted: false,
        }
    }

    /// The state Undo would return to.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            project: self.project.clone(),
            active: self.active,
        }
    }

    /// Puts a snapshot back.
    pub fn restore(&mut self, s: Snapshot) {
        self.project = s.project;
        self.active = s.active;
    }

    /// Takes the project, active document and file of `worked` (a clone that ran a tool),
    /// recording the state before it as one Undo step named `label`.
    pub fn commit(&mut self, label: &str, worked: Session) {
        let before = self.snapshot();
        self.history.break_coalescing();
        self.history.record(label, before);
        self.project = worked.project;
        self.active = worked.active;
        self.path = worked.path;
        self.dirty = worked.dirty;
    }

    /// The document named `name` (case-insensitive), or the active one when `name` is `None`.
    pub fn doc_id(&self, cmd: &str, name: Option<&str>) -> Result<DocumentId, ApiError> {
        match name {
            Some(n) => self
                .project
                .documents()
                .iter()
                .find(|d| d.name.eq_ignore_ascii_case(n))
                .map(|d| d.id)
                .ok_or_else(|| ApiError::new(cmd, format!("no document named '{n}'"))),
            None => self.active.ok_or_else(|| {
                ApiError::new(cmd, "no active document: call document_new or project_open")
            }),
        }
    }

    /// The part `name` (or the active document, which must be a part).
    pub fn part(&self, cmd: &str, name: Option<&str>) -> Result<&PartDocument, ApiError> {
        let id = self.doc_id(cmd, name)?;
        match self.project.document(id).map(|d| &d.data) {
            Some(DocumentData::Part(p)) => Ok(p),
            _ => Err(ApiError::new(cmd, "the document is not a part")),
        }
    }

    /// The part `name` (or the active one), to change.
    pub fn part_mut(
        &mut self,
        cmd: &str,
        name: Option<&str>,
    ) -> Result<&mut PartDocument, ApiError> {
        let id = self.doc_id(cmd, name)?;
        match self.project.document_mut(id).map(|d| &mut d.data) {
            Some(DocumentData::Part(p)) => Ok(p),
            _ => Err(ApiError::new(cmd, "the document is not a part")),
        }
    }

    /// The assembly `name` (or the active one).
    pub fn assembly(&self, cmd: &str, name: Option<&str>) -> Result<&AssemblyDocument, ApiError> {
        let id = self.doc_id(cmd, name)?;
        match self.project.document(id).map(|d| &d.data) {
            Some(DocumentData::Assembly(a)) => Ok(a),
            _ => Err(ApiError::new(cmd, "the document is not an assembly")),
        }
    }

    /// The assembly `name` (or the active one), to change.
    pub fn assembly_mut(
        &mut self,
        cmd: &str,
        name: Option<&str>,
    ) -> Result<&mut AssemblyDocument, ApiError> {
        let id = self.doc_id(cmd, name)?;
        match self.project.document_mut(id).map(|d| &mut d.data) {
            Some(DocumentData::Assembly(a)) => Ok(a),
            _ => Err(ApiError::new(cmd, "the document is not an assembly")),
        }
    }
}
