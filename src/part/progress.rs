//! How far a long job has got (spec 0.19 §3.1): the GUI shows a bar for a job that runs on a
//! worker thread, and the regeneration reports to it feature by feature.
//!
//! A job installs a [`Reporter`] on its own thread ([`Reporter::install`]); [`feature`] and
//! [`stage`] write into the reporter of the calling thread and do nothing on a thread without
//! one (tests, the main thread, the API). [`running`] lists the jobs whose reporter is still
//! installed, longest-running first.

use std::cell::RefCell;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

/// What a job has reported so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// What the job is (`"Pattern1"` for a preview, `"Rebuild"`).
    pub job: String,
    /// The feature being built, its position (from 1) and the number of features.
    pub feature: Option<(String, usize, usize)>,
    /// A stage of that feature (`"cutting 5041 holes"`).
    pub stage: Option<String>,
    /// How long the job has run.
    pub elapsed: Duration,
}

impl Snapshot {
    /// The fraction of the features done, `0..=1`; `None` before the first feature.
    pub fn fraction(&self) -> Option<f32> {
        let (_, k, n) = self.feature.as_ref()?;
        // a feature counts as done when the next one starts
        Some(if *n == 0 {
            1.0
        } else {
            (*k - 1) as f32 / *n as f32
        })
    }

    /// One line for the status bar: `Pattern1: building Extrusion2 (2 of 5), cutting 36 holes`.
    pub fn text(&self) -> String {
        let mut s = self.job.clone();
        if let Some((name, k, n)) = &self.feature {
            s.push_str(&format!(": building {name} ({k} of {n})"));
        }
        if let Some(stage) = &self.stage {
            s.push_str(", ");
            s.push_str(stage);
        }
        s
    }
}

#[derive(Debug)]
struct State {
    job: String,
    started: Instant,
    feature: Option<(String, usize, usize)>,
    stage: Option<String>,
}

type Shared = Arc<Mutex<State>>;

static RUNNING: Mutex<Vec<Weak<Mutex<State>>>> = Mutex::new(Vec::new());

thread_local! {
    static CURRENT: RefCell<Option<Shared>> = const { RefCell::new(None) };
}

/// A job's report, installed on the thread doing its work for as long as the value lives.
#[derive(Debug)]
pub struct Reporter {
    state: Shared,
    previous: Option<Shared>,
}

impl Reporter {
    /// Starts reporting the job `job` from this thread (until the reporter is dropped).
    pub fn install(job: &str) -> Self {
        let state = Arc::new(Mutex::new(State {
            job: job.to_owned(),
            started: Instant::now(),
            feature: None,
            stage: None,
        }));
        if let Ok(mut all) = RUNNING.lock() {
            all.retain(|w| w.strong_count() > 0);
            all.push(Arc::downgrade(&state));
        }
        let previous = CURRENT.with(|c| c.replace(Some(state.clone())));
        Self { state, previous }
    }
}

impl Drop for Reporter {
    fn drop(&mut self) {
        let previous = self.previous.take();
        CURRENT.with(|c| *c.borrow_mut() = previous);
        if let Ok(mut all) = RUNNING.lock() {
            let me = Arc::downgrade(&self.state);
            all.retain(|w| w.strong_count() > 0 && !w.ptr_eq(&me));
        }
    }
}

fn with_current(f: impl FnOnce(&mut State)) {
    CURRENT.with(|c| {
        if let Some(s) = c.borrow().as_ref()
            && let Ok(mut st) = s.lock()
        {
            f(&mut st);
        }
    });
}

/// The calling thread's job is building feature `name`, `k` of `n` (from 1).
pub fn feature(name: &str, k: usize, n: usize) {
    with_current(|s| {
        s.feature = Some((name.to_owned(), k, n));
        s.stage = None;
    });
}

/// The calling thread's job has reached `stage` of its feature.
pub fn stage(stage: &str) {
    with_current(|s| s.stage = Some(stage.to_owned()));
}

/// The jobs whose reporter is installed, the longest-running first.
pub fn running() -> Vec<Snapshot> {
    let Ok(all) = RUNNING.lock() else {
        return Vec::new();
    };
    let mut out: Vec<Snapshot> = all
        .iter()
        .filter_map(Weak::upgrade)
        .filter_map(|s| {
            let st = s.lock().ok()?;
            Some(Snapshot {
                job: st.job.clone(),
                feature: st.feature.clone(),
                stage: st.stage.clone(),
                elapsed: st.started.elapsed(),
            })
        })
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.elapsed));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mine(job: &str) -> Option<Snapshot> {
        running().into_iter().find(|s| s.job == job)
    }

    #[test]
    fn a_reporter_lives_on_its_thread_only() {
        // nothing is reported on a thread without a reporter
        feature("Extrusion1", 1, 2);
        let job = "progress test A";
        let r = Reporter::install(job);
        feature("Extrusion1", 1, 2);
        stage("cutting 3 holes");
        let s = mine(job).unwrap();
        assert_eq!(s.feature, Some(("Extrusion1".into(), 1, 2)));
        assert_eq!(
            s.text(),
            "progress test A: building Extrusion1 (1 of 2), cutting 3 holes"
        );
        assert_eq!(s.fraction(), Some(0.0));
        // a new feature clears the stage
        feature("Pattern1", 2, 2);
        let s = mine(job).unwrap();
        assert_eq!(s.stage, None);
        assert_eq!(s.fraction(), Some(0.5));
        // another thread's reports do not reach it
        std::thread::spawn(|| feature("Other", 9, 9))
            .join()
            .unwrap();
        assert_eq!(mine(job).unwrap().feature, Some(("Pattern1".into(), 2, 2)));
        drop(r);
        assert!(mine(job).is_none());
    }

    #[test]
    fn a_job_on_a_worker_is_listed_while_it_runs() {
        let job = "progress test B";
        let (go, wait) = std::sync::mpsc::channel::<()>();
        let (ready, started) = std::sync::mpsc::channel::<()>();
        let t = std::thread::spawn(move || {
            let _r = Reporter::install(job);
            feature("Hole1", 3, 4);
            ready.send(()).unwrap();
            wait.recv().unwrap();
        });
        started.recv().unwrap();
        assert_eq!(mine(job).unwrap().feature, Some(("Hole1".into(), 3, 4)));
        go.send(()).unwrap();
        t.join().unwrap();
        assert!(mine(job).is_none());
    }
}
