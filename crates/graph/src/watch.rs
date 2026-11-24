//! Keeps an [`Index`] in step with the graph folder using a `notify` file
//! watcher. Events are debounced: a burst of writes (an editor saving, a
//! sync client unpacking) is indexed once, after the folder goes quiet.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use notify::event::{AccessKind, AccessMode};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::index::Index;
use crate::layout::{Graph, GraphError, CONFIG_PATH, JOURNALS_DIR, PAGES_DIR};

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
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                // Linux reports read opens, including reads from our own index
                // pass. They must not trigger or extend the debounce window.
                if !event.as_ref().is_ok_and(|event| {
                    matches!(
                        event.kind,
                        EventKind::Access(
                            AccessKind::Open(_)
                                | AccessKind::Read
                                | AccessKind::Close(AccessMode::Read)
                        )
                    )
                }) {
                    let _ = tx.send(event);
                }
            })
            .map_err(io_error)?;
        watcher
            .watch(graph.root(), RecursiveMode::Recursive)
            .map_err(io_error)?;
        let thread = std::thread::Builder::new()
            .name("graph-watcher".into())
            .spawn(move || run(graph, &index, debounce, &rx, on_change.as_ref()))
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

/// A notify error's kind as a log token.
fn notify_kind(kind: &notify::ErrorKind) -> &'static str {
    match kind {
        notify::ErrorKind::Generic(_) => "generic",
        notify::ErrorKind::Io(_) => "io",
        notify::ErrorKind::PathNotFound => "path_not_found",
        notify::ErrorKind::WatchNotFound => "watch_not_found",
        notify::ErrorKind::InvalidConfig(_) => "invalid_config",
        notify::ErrorKind::MaxFilesWatch => "max_files_watch",
    }
}

#[allow(clippy::needless_pass_by_value)] // used as `map_err(io_error)`
fn io_error(e: notify::Error) -> GraphError {
    GraphError::Io(std::io::Error::other(e.to_string()))
}

fn run(
    mut graph: Graph,
    index: &Mutex<Index>,
    debounce: Duration,
    rx: &mpsc::Receiver<notify::Result<notify::Event>>,
    on_change: Option<&OnChange>,
) {
    loop {
        let Ok(first) = rx.recv() else { return };
        let mut paths: BTreeSet<PathBuf> = BTreeSet::new();
        let mut collect = |event: notify::Result<notify::Event>| match event {
            Ok(event) => paths.extend(event.paths),
            // The error can name a path: log its kind only.
            Err(error) => tracing::warn!(
                target: "notes",
                kind = notify_kind(&error.kind),
                "watcher_error"
            ),
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
        flush(&mut graph, index, &paths, on_change);
        if closed {
            return;
        }
    }
}

fn flush(
    graph: &mut Graph,
    index: &Mutex<Index>,
    paths: &BTreeSet<PathBuf>,
    on_change: Option<&OnChange>,
) {
    let mut files: Vec<String> = Vec::new();
    let mut full_sync = false;
    for path in paths {
        let Some(rel) = graph.relative(path) else {
            continue;
        };
        if rel == CONFIG_PATH {
            // `sync` rebuilds by itself when the settings changed.
            graph.reload_config();
            full_sync = true;
        } else if graph.is_ignored(&rel) {
            // Hidden, backup or temp file.
        } else if graph.classify(&rel).is_some() {
            files.push(rel);
        } else if rel == PAGES_DIR
            || rel == JOURNALS_DIR
            || rel.starts_with("pages/")
            || rel.starts_with("journals/")
        {
            // A folder inside the layout appeared, moved or went away (one
            // event for the whole folder): let sync work out what changed.
            full_sync = true;
        }
    }
    if files.is_empty() && !full_sync {
        return;
    }
    let Ok(mut idx) = index.lock() else { return };
    let started = Instant::now();
    // Counts and kinds only: a file's name is a page's name.
    let (mode, failed) = if full_sync {
        ("sync", usize::from(idx.sync(graph).is_err()))
    } else {
        let failed = files
            .iter()
            .filter(|rel| idx.index_file(graph, rel).is_err())
            .count();
        ("files", failed)
    };
    drop(idx);
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    if failed > 0 {
        tracing::warn!(target: "notes", kind = "reindex", mode, count = failed, "watcher_error");
    }
    tracing::debug!(target: "notes", mode, files = files.len(), duration_ms, "graph_reindexed");
    if full_sync {
        // The listener hears "everything may have changed" as no files.
        files.clear();
    }
    if let Some(cb) = on_change {
        cb(&files);
    }
}
