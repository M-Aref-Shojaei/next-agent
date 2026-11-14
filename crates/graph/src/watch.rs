//! Keeps an [`Index`] in step with the graph folder using a `notify` file
//! watcher. Events are debounced: a burst of writes (an editor saving, a
//! sync client unpacking) is indexed once, after the folder goes quiet.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::index::Index;
use crate::layout::{Graph, GraphError, CONFIG_PATH};

/// Called after each debounced batch with the graph-relative paths that
/// were re-indexed (empty when a full sync ran instead).
pub type OnChange = Box<dyn Fn(&[String]) + Send + 'static>;

/// A running watcher. Dropping it stops the watcher and its thread.
pub struct GraphWatcher {
    watcher: Option<RecommendedWatcher>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for GraphWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphWatcher").finish_non_exhaustive()
    }
}

impl GraphWatcher {
    /// Watch `graph`'s folder and re-index changed files into `index`.
    ///
    /// # Errors
    /// [`GraphError::Io`] when the OS watcher cannot start.
    pub fn start(
        graph: Graph,
        index: Arc<Mutex<Index>>,
        debounce: Duration,
        on_change: Option<OnChange>,
    ) -> Result<Self, GraphError> {
        let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = tx.send(event);
        })
        .map_err(io_error)?;
        watcher
            .watch(graph.root(), RecursiveMode::Recursive)
            .map_err(io_error)?;
        let canonical = graph
            .root()
            .canonicalize()
            .unwrap_or_else(|_| graph.root().to_path_buf());
        let thread = std::thread::Builder::new()
            .name("graph-watcher".into())
            .spawn(move || run(graph, &canonical, &index, debounce, &rx, on_change.as_ref()))
            .map_err(GraphError::Io)?;
        Ok(Self {
            watcher: Some(watcher),
            thread: Some(thread),
        })
    }
}

impl Drop for GraphWatcher {
    fn drop(&mut self) {
        // Dropping the OS watcher closes the channel, which ends the thread.
        drop(self.watcher.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // used as `map_err(io_error)`
fn io_error(e: notify::Error) -> GraphError {
    GraphError::Io(std::io::Error::other(e.to_string()))
}

fn run(
    mut graph: Graph,
    canonical: &std::path::Path,
    index: &Mutex<Index>,
    debounce: Duration,
    rx: &mpsc::Receiver<notify::Result<notify::Event>>,
    on_change: Option<&OnChange>,
) {
    loop {
        let Ok(first) = rx.recv() else { return };
        let mut paths: BTreeSet<PathBuf> = BTreeSet::new();
        let mut collect = |event: notify::Result<notify::Event>| {
            if let Ok(event) = event {
                paths.extend(event.paths);
            }
        };
        collect(first);
        let mut closed = false;
        loop {
            match rx.recv_timeout(debounce) {
                Ok(event) => collect(event),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    closed = true;
                    break;
                }
            }
        }
        flush(&mut graph, canonical, index, &paths, on_change);
        if closed {
            return;
        }
    }
}

fn flush(
    graph: &mut Graph,
    canonical: &std::path::Path,
    index: &Mutex<Index>,
    paths: &BTreeSet<PathBuf>,
    on_change: Option<&OnChange>,
) {
    let mut files: Vec<String> = Vec::new();
    let mut full_sync = false;
    for path in paths {
        let rel = graph.relative(path).or_else(|| {
            path.strip_prefix(canonical)
                .ok()
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .filter(|r| !r.is_empty())
        });
        let Some(rel) = rel else { continue };
        if rel == CONFIG_PATH {
            graph.reload_config();
            full_sync = true;
        } else if graph.is_ignored(&rel) {
            // Hidden, backup or temp file.
        } else if graph.classify(&rel).is_some() {
            files.push(rel);
        } else if path.is_dir() || (rel == "pages" || rel == "journals") {
            // A folder moved or appeared: let sync work out what changed.
            full_sync = true;
        }
    }
    if files.is_empty() && !full_sync {
        return;
    }
    let Ok(mut idx) = index.lock() else { return };
    if full_sync {
        let _ = idx.sync(graph);
        files.clear();
    } else {
        for rel in &files {
            let _ = idx.index_file(graph, rel);
        }
    }
    drop(idx);
    if let Some(cb) = on_change {
        cb(&files);
    }
}
