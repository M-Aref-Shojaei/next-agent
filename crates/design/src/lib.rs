//! AI Design Studio mechanics, UI-independent and testable without a window.
//!
//! Our own UX; mechanics ported from three Apache-2.0 projects (pinned in
//! `upstream.lock.json`, credited in `NOTICE.md`, each derived file carries a
//! header saying what changed):
//! - **Open Design** (`59e4f2a`): `DESIGN.md` design systems ([`system`]),
//!   `<artifact>` parsing ([`artifact`]), design directions and the
//!   five-dimension self-critique ([`prompts`], [`critique`]), artifact lint
//!   ([`lint`]), sandboxed previews and the SSRF guard ([`sandbox`], [`ssrf`]).
//! - **Onlook** (`423e2e9`): element → source mapping ([`source_map`]) and
//!   visual edits written back to code ([`codemod`]).
//! - **`OpenUI`** (`42d7ab4`): HTML → framework conversion ([`convert`]).
//!
//! The model is behind the [`DesignModel`] seam: the desktop app adapts the
//! LLM gateway's `design` role to it; [`mock::MockDesignModel`] runs the whole
//! studio offline. Code writes are **not** done here: callers turn a
//! [`codemod::StyleEdit`] or [`convert::Converted`] into tool calls that go
//! through the broker and permission engine inside a task worktree.

pub mod artifact;
pub mod codemod;
pub mod convert;
pub mod critique;
pub mod css;
pub mod lint;
pub mod mock;
pub mod prompts;
pub mod proxy;
pub mod sandbox;
pub mod sketch;
pub mod source_map;
pub mod ssrf;
pub mod studio;
pub mod system;

pub use artifact::Artifact;
pub use studio::{DesignModel, Direction, Frame, GenerateRequest, IterateRequest, Progress, Turn};
pub use system::{DesignSystem, Token, TokenKind};

/// Errors from design-studio mechanics.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DesignError {
    #[error("{0}")]
    Io(String),
    #[error("path is outside the workspace: {0}")]
    Outside(String),
    #[error("not an editable source file: {0}")]
    NotSource(String),
    #[error("could not find {0}")]
    NotFound(String),
    #[error("refused unsafe style {0}")]
    BadStyle(String),
    #[error("can't merge into {0}; edit this element's style by hand")]
    Unsupported(String),
    #[error("blocked URL: {0}")]
    Blocked(String),
    #[error("design model: {0}")]
    Model(String),
}
