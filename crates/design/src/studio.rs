//! The studio's flows over the [`DesignModel`] seam: generate directions
//! (with self-critique), refine one by chat, convert one to framework code.

use crate::artifact::{self, Artifact};
use crate::convert::{self, Converted, Framework};
use crate::critique::{self, Critique};
use crate::lint::{self, Finding};
use crate::prompts;
use crate::system;
use crate::DesignError;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Fewest directions a generation run returns.
const MIN_DIRECTIONS: usize = 3;
/// Most directions a generation run returns.
const MAX_DIRECTIONS: usize = 5;

pub use crate::sandbox::FrameKind as Frame;

/// One chat turn sent to the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    /// `user` or `assistant`.
    pub role: String,
    pub text: String,
}

impl Turn {
    #[must_use]
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            text: text.into(),
        }
    }
    #[must_use]
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            text: text.into(),
        }
    }
}

/// The `design` model role, as the studio sees it.
pub trait DesignModel: Send + Sync {
    /// One completion: system prompt plus turns in, reply text out.
    ///
    /// # Errors
    /// Transport, auth or budget failures, as a message.
    fn complete(&self, system: &str, turns: &[Turn]) -> Result<String, String>;
}

/// A generated design direction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Direction {
    pub identifier: String,
    pub title: String,
    pub rationale: String,
    pub html: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub critique: Option<Critique>,
    #[serde(default)]
    pub lint: Vec<Finding>,
}

/// Progress reported while generating.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum Progress {
    Generating {
        count: usize,
    },
    Critiquing {
        index: usize,
        total: usize,
        title: String,
    },
    Done,
}

pub struct GenerateRequest<'a> {
    pub prompt: &'a str,
    pub frame: Frame,
    /// Clamped to 3–5.
    pub count: usize,
    pub design_md: &'a str,
    pub tokens_css: &'a str,
    /// Text wireframe from [`crate::sketch::describe`].
    pub sketch: Option<&'a str>,
    /// Run the self-critique pass (on by default in the UI).
    pub critique: bool,
}

pub struct IterateRequest<'a> {
    pub design_md: &'a str,
    pub tokens_css: &'a str,
    pub frame: Frame,
    pub current: &'a Direction,
    /// Earlier change requests and replies for this direction.
    pub history: &'a [Turn],
    pub message: &'a str,
}

fn token_colors(tokens_css: &str) -> Vec<String> {
    let s = system::extract_from_sources("tokens", &[("tokens.css".into(), tokens_css.into())]);
    s.tokens
        .iter()
        .flat_map(|t| {
            std::iter::once(t.value.to_ascii_lowercase())
                .chain(t.dark.iter().map(|d| d.to_ascii_lowercase()))
        })
        .collect()
}

fn html_artifacts(reply: &str) -> Vec<Artifact> {
    artifact::parse(reply)
        .into_iter()
        .filter(|a| a.kind.contains("html") && !a.content.trim().is_empty())
        .collect()
}

/// `Direction: <title> — <why>` lines, in order.
fn rationales(reply: &str) -> Vec<String> {
    artifact::prose(reply)
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Direction:"))
        .map(|l| {
            l.split_once('—')
                .or_else(|| l.split_once(" - "))
                .map_or(l, |(_, why)| why)
                .trim()
                .to_string()
        })
        .collect()
}

/// Generate [`MIN_DIRECTIONS`]–[`MAX_DIRECTIONS`] directions, lint them, and (optionally) run the
/// five-dimension self-critique on each before returning.
///
/// # Errors
/// [`DesignError::Model`] when the model fails or returns no design.
pub fn generate(
    model: &dyn DesignModel,
    req: &GenerateRequest<'_>,
    progress: &mut dyn FnMut(Progress),
) -> Result<Vec<Direction>, DesignError> {
    let count = req.count.clamp(MIN_DIRECTIONS, MAX_DIRECTIONS);
    progress(Progress::Generating { count });
    let system = prompts::generate_system(req.design_md, req.tokens_css, req.frame, count);
    let reply = model
        .complete(
            &system,
            &[Turn::user(prompts::generate_user(req.prompt, req.sketch))],
        )
        .map_err(DesignError::Model)?;
    let artifacts = html_artifacts(&reply);
    if artifacts.is_empty() {
        return Err(DesignError::Model(
            "the reply contained no design (no <artifact> block)".into(),
        ));
    }
    let whys = rationales(&reply);
    let colors = token_colors(req.tokens_css);
    let mut directions: Vec<Direction> = artifacts
        .into_iter()
        .take(count)
        .enumerate()
        .map(|(i, a)| Direction {
            title: if a.title.is_empty() {
                format!("Direction {}", i + 1)
            } else {
                a.title.clone()
            },
            rationale: whys.get(i).cloned().unwrap_or_default(),
            lint: lint::lint(&a.content, &colors),
            identifier: a.identifier,
            html: a.content,
            critique: None,
        })
        .collect();
    if req.critique {
        let total = directions.len();
        let system = prompts::critique_system(req.design_md, req.tokens_css);
        for (index, d) in directions.iter_mut().enumerate() {
            progress(Progress::Critiquing {
                index,
                total,
                title: d.title.clone(),
            });
            let lint_lines: Vec<String> = d
                .lint
                .iter()
                .map(|f| format!("{:?} {}: {}", f.severity, f.rule, f.message))
                .collect();
            let user =
                prompts::critique_user(req.prompt, &d.identifier, &d.title, &d.html, &lint_lines);
            // A failed review keeps the direction unreviewed rather than failing the batch.
            let Ok(reply) = model.complete(&system, &[Turn::user(user)]) else {
                continue;
            };
            if let Some((mut c, revision)) = critique::parse(&reply) {
                if let Some(rev) = revision {
                    d.html = rev.content;
                    d.lint = lint::lint(&d.html, &colors);
                    c.revised = true;
                }
                d.critique = Some(c);
            }
        }
    }
    progress(Progress::Done);
    Ok(directions)
}

/// Apply one chat change to a direction. Returns the new version and the
/// model's one-line summary.
///
/// # Errors
/// [`DesignError::Model`] when the model fails or returns no design.
pub fn iterate(
    model: &dyn DesignModel,
    req: &IterateRequest<'_>,
) -> Result<(Direction, String), DesignError> {
    let system = prompts::iterate_system(req.design_md, req.tokens_css, req.frame);
    let mut turns: Vec<Turn> = req.history.to_vec();
    turns.push(Turn::user(prompts::iterate_user(
        &req.current.identifier,
        &req.current.title,
        &req.current.html,
        req.message,
    )));
    let reply = model
        .complete(&system, &turns)
        .map_err(DesignError::Model)?;
    let a = html_artifacts(&reply)
        .into_iter()
        .next()
        .ok_or_else(|| DesignError::Model("the reply contained no updated design".into()))?;
    let summary = artifact::prose(&reply)
        .lines()
        .next()
        .unwrap_or("Updated.")
        .trim()
        .to_string();
    let colors = token_colors(req.tokens_css);
    Ok((
        Direction {
            identifier: req.current.identifier.clone(),
            title: req.current.title.clone(),
            rationale: req.current.rationale.clone(),
            lint: lint::lint(&a.content, &colors),
            html: a.content,
            critique: None,
        },
        summary,
    ))
}

/// Convert a direction to `framework` code for `root`'s layout.
///
/// # Errors
/// [`DesignError::Model`] when the model fails or replies with nothing.
pub fn convert(
    model: &dyn DesignModel,
    root: &Path,
    direction: &Direction,
    framework: Framework,
    design_md: &str,
) -> Result<Converted, DesignError> {
    let component = convert::component_name(&direction.title);
    let file_name = framework.file_name(&component);
    let system =
        prompts::convert_system(framework.label(), framework.mime(), &file_name, design_md);
    let reply = model
        .complete(
            &system,
            &[Turn::user(prompts::convert_user(
                framework.label(),
                &direction.html,
            ))],
        )
        .map_err(DesignError::Model)?;
    let contents = convert::extract_code(&reply);
    if contents.trim().is_empty() {
        return Err(DesignError::Model("the conversion came back empty".into()));
    }
    Ok(Converted {
        framework,
        path: convert::target_path(root, framework, &component),
        contents,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Replies in order and records the system prompts it saw.
    struct Scripted {
        replies: Mutex<Vec<Result<String, String>>>,
        seen: Mutex<Vec<String>>,
    }

    impl Scripted {
        fn new(replies: Vec<Result<&str, &str>>) -> Self {
            Self {
                replies: Mutex::new(
                    replies
                        .into_iter()
                        .rev()
                        .map(|r| r.map(str::to_string).map_err(str::to_string))
                        .collect(),
                ),
                seen: Mutex::default(),
            }
        }
    }

    impl DesignModel for Scripted {
        fn complete(&self, system: &str, turns: &[Turn]) -> Result<String, String> {
            self.seen.lock().expect("lock").push(format!(
                "{system}\n---\n{}",
                turns.last().map_or("", |t| t.text.as_str())
            ));
            self.replies
                .lock()
                .expect("lock")
                .pop()
                .unwrap_or_else(|| Err("no more".into()))
        }
    }

    const TOKENS: &str = ":root { --accent: #3b5bdb; }";

    fn req(critique: bool) -> GenerateRequest<'static> {
        GenerateRequest {
            prompt: "Settings page",
            frame: Frame::Web,
            count: 9,
            design_md: "# DS",
            tokens_css: TOKENS,
            sketch: None,
            critique,
        }
    }

    #[test]
    fn generates_and_critiques() {
        let model = Scripted::new(vec![
            Ok("Direction: Calm — quiet\n<artifact identifier=\"modern-minimal\" title=\"Calm\"><html>a</html></artifact>\nDirection: Dense - more data\n<artifact identifier=\"tech-utility\" title=\"Dense\"><html style=\"color:#ff0000\">b</html></artifact>\n<artifact identifier=\"x\" type=\"text/jsx\">nope</artifact>"),
            Ok("{\"philosophy\":4,\"hierarchy\":4,\"execution\":4,\"specificity\":4,\"restraint\":4,\"notes\":\"ok\"}"),
            Ok("{\"philosophy\":4,\"hierarchy\":2,\"execution\":4,\"specificity\":4,\"restraint\":4,\"notes\":\"fix\"}<artifact identifier=\"tech-utility\" title=\"Dense\"><html>b2</html></artifact>"),
        ]);
        let mut phases = Vec::new();
        let d = generate(&model, &req(true), &mut |p| phases.push(p)).expect("ok");
        assert_eq!(d.len(), 2, "jsx artifact ignored");
        assert_eq!(d[0].rationale, "quiet");
        assert_eq!(d[1].rationale, "more data");
        assert!(d[0].critique.as_ref().expect("c").passes());
        assert!(d[1].critique.as_ref().expect("c").revised);
        assert_eq!(d[1].html, "<html>b2</html>");
        assert!(
            !d[1].lint.iter().any(|f| f.rule == "off-palette"),
            "re-linted after revision"
        );
        assert_eq!(phases.first(), Some(&Progress::Generating { count: 5 }));
        assert_eq!(phases.last(), Some(&Progress::Done));
        assert!(model.seen.lock().expect("l")[0].contains("Design 5 clearly different directions"));
    }

    #[test]
    fn review_failures_keep_directions() {
        let model = Scripted::new(vec![
            Ok("<artifact identifier=\"a\"><p>x</p></artifact>"),
            Err("rate limited"),
            Ok("no json"),
        ]);
        let d = generate(&model, &req(true), &mut |_| {}).expect("ok");
        assert_eq!(d[0].title, "Direction 1");
        assert!(d[0].critique.is_none());
    }

    #[test]
    fn empty_or_failed_generation_is_an_error() {
        assert!(matches!(
            generate(&Scripted::new(vec![Ok("sorry")]), &req(false), &mut |_| {}),
            Err(DesignError::Model(_))
        ));
        assert!(
            matches!(generate(&Scripted::new(vec![Err("401")]), &req(false), &mut |_| {}), Err(DesignError::Model(m)) if m == "401")
        );
    }

    #[test]
    fn iterates_with_history() {
        let model = Scripted::new(vec![Ok("Made the header sticky.\n<artifact identifier=\"a\" title=\"A\"><html>v2</html></artifact>")]);
        let current = Direction {
            identifier: "a".into(),
            title: "A".into(),
            rationale: String::new(),
            html: "<html>v1</html>".into(),
            critique: None,
            lint: vec![],
        };
        let history = vec![Turn::user("earlier"), Turn::assistant("done")];
        let (next, summary) = iterate(
            &model,
            &IterateRequest {
                design_md: "",
                tokens_css: TOKENS,
                frame: Frame::Mobile,
                current: &current,
                history: &history,
                message: "sticky header",
            },
        )
        .expect("ok");
        assert_eq!(next.html, "<html>v2</html>");
        assert_eq!(summary, "Made the header sticky.");
        let seen = &model.seen.lock().expect("l")[0];
        assert!(seen.contains("<html>v1</html>") && seen.contains("Change: sticky header"));
    }

    #[test]
    fn converts_to_a_file_in_the_project_layout() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::create_dir_all(dir.path().join("src/lib")).expect("mk");
        let model = Scripted::new(vec![Ok("<artifact identifier=\"CalmSettings.svelte\" type=\"text/x-svelte\"><h1>x</h1></artifact>")]);
        let d = Direction {
            identifier: "a".into(),
            title: "Calm settings".into(),
            rationale: String::new(),
            html: "<h1>x</h1>".into(),
            critique: None,
            lint: vec![],
        };
        let c = convert(&model, dir.path(), &d, Framework::Svelte, "").expect("ok");
        assert_eq!(c.path, "src/lib/components/CalmSettings.svelte");
        assert_eq!(c.contents, "<h1>x</h1>\n");
    }
}
