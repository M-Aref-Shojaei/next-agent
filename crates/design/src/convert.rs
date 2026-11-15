// Adapted from OpenUI (Apache-2.0, © 2024 Weights and Biases, Inc.):
// frontend/src/api/openai.ts (`convert`) and frontend/src/state/atoms/
// history.ts (framework list) @ 42d7ab4 — modified: Rust; the target
// framework is detected from the workspace's package.json, the reply is one
// `<artifact>` file, and the output path follows the project's layout. The
// file is written by a task in a worktree, never here.
//! HTML direction → Svelte, React, Vue or Web Component source file.

use crate::artifact;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Framework {
    Svelte,
    React,
    Vue,
    WebComponent,
}

impl Framework {
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "svelte" => Some(Self::Svelte),
            "react" => Some(Self::React),
            "vue" => Some(Self::Vue),
            "web-component" | "webcomponent" | "web component" => Some(Self::WebComponent),
            _ => None,
        }
    }

    /// The name used in the prompt.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Svelte => "Svelte 5 (runes: $props, $state)",
            Self::React => "React (function components, TypeScript)",
            Self::Vue => "Vue 3 (<script setup lang=\"ts\">)",
            Self::WebComponent => {
                "standard Web Component (custom element with shadow DOM, no framework)"
            }
        }
    }

    #[must_use]
    pub fn mime(self) -> &'static str {
        match self {
            Self::Svelte => "text/x-svelte",
            Self::React => "text/tsx",
            Self::Vue => "text/x-vue",
            Self::WebComponent => "text/javascript",
        }
    }

    #[must_use]
    pub fn file_name(self, component: &str) -> String {
        match self {
            Self::Svelte => format!("{component}.svelte"),
            Self::React => format!("{component}.tsx"),
            Self::Vue => format!("{component}.vue"),
            Self::WebComponent => format!("{}.js", kebab(component)),
        }
    }
}

/// The project's framework from `package.json` dependencies (Svelte, then
/// React, then Vue; otherwise Web Components).
#[must_use]
pub fn detect(root: &Path) -> Framework {
    let Ok(text) = std::fs::read_to_string(root.join("package.json")) else {
        return Framework::WebComponent;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Framework::WebComponent;
    };
    let has = |dep: &str| {
        ["dependencies", "devDependencies", "peerDependencies"]
            .iter()
            .any(|k| json.get(k).and_then(|d| d.get(dep)).is_some())
    };
    if has("svelte") || has("@sveltejs/kit") {
        Framework::Svelte
    } else if has("react") || has("next") {
        Framework::React
    } else if has("vue") || has("nuxt") {
        Framework::Vue
    } else {
        Framework::WebComponent
    }
}

/// `PascalCase` component name from a direction title (ASCII, ≤ 40 chars).
#[must_use]
pub fn component_name(title: &str) -> String {
    let mut out = String::new();
    for word in title.split(|c: char| !c.is_ascii_alphanumeric()) {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.extend(chars);
        }
    }
    let out: String = out
        .trim_start_matches(|c: char| c.is_ascii_digit())
        .chars()
        .take(40)
        .collect();
    if out.is_empty() {
        "DesignDirection".into()
    } else {
        out
    }
}

fn kebab(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('-');
        }
        out.push(c.to_ascii_lowercase());
    }
    if out.contains('-') {
        out
    } else {
        format!("{out}-view") // custom element names need a hyphen
    }
}

/// Workspace-relative output path: `src/lib/components/` in a `SvelteKit`
/// project, `src/components/` when `src/` exists, else `components/`.
#[must_use]
pub fn target_path(root: &Path, framework: Framework, component: &str) -> String {
    let dir = if root.join("src/lib").is_dir() {
        "src/lib/components"
    } else if root.join("src").is_dir() {
        "src/components"
    } else {
        "components"
    };
    format!("{dir}/{}", framework.file_name(component))
}

/// A converted file ready to be written by a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Converted {
    pub framework: Framework,
    pub path: String,
    pub contents: String,
}

/// The code from the model's reply: the first artifact, else the first
/// fenced block, else the whole reply.
#[must_use]
pub fn extract_code(reply: &str) -> String {
    if let Some(a) = artifact::parse(reply)
        .into_iter()
        .find(|a| !a.content.is_empty() && !a.kind.contains("html"))
    {
        return strip_fence(&a.content);
    }
    strip_fence(reply)
}

fn strip_fence(text: &str) -> String {
    let t = text.trim();
    if let Some(start) = t.find("```") {
        let body = t[start + 3..]
            .find('\n')
            .map_or(t.len(), |n| start + 3 + n + 1);
        let end = t[body..].find("```").map_or(t.len(), |e| body + e);
        return format!("{}\n", t[body..end].trim_end());
    }
    format!("{t}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_framework_from_package_json() {
        let dir = tempfile::tempdir().expect("tmp");
        assert_eq!(detect(dir.path()), Framework::WebComponent);
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies":{"@sveltejs/kit":"2"}}"#,
        )
        .expect("w");
        assert_eq!(detect(dir.path()), Framework::Svelte);
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies":{"react":"19","react-dom":"19"}}"#,
        )
        .expect("w");
        assert_eq!(detect(dir.path()), Framework::React);
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies":{"vue":"3"}}"#,
        )
        .expect("w");
        assert_eq!(detect(dir.path()), Framework::Vue);
        std::fs::write(dir.path().join("package.json"), "not json").expect("w");
        assert_eq!(detect(dir.path()), Framework::WebComponent);
    }

    #[test]
    fn names_and_paths() {
        assert_eq!(component_name("Calm settings — v2!"), "CalmSettingsV2");
        assert_eq!(component_name("../../etc"), "Etc");
        assert_eq!(component_name("42"), "DesignDirection");
        assert_eq!(Framework::WebComponent.file_name("Login"), "login-view.js");
        assert_eq!(
            Framework::WebComponent.file_name("LoginCard"),
            "login-card.js"
        );
        let dir = tempfile::tempdir().expect("tmp");
        assert_eq!(
            target_path(dir.path(), Framework::React, "A"),
            "components/A.tsx"
        );
        std::fs::create_dir_all(dir.path().join("src/lib")).expect("mk");
        assert_eq!(
            target_path(dir.path(), Framework::Svelte, "A"),
            "src/lib/components/A.svelte"
        );
    }

    #[test]
    fn extracts_code_from_replies() {
        assert_eq!(
            extract_code(
                "<artifact identifier=\"A.svelte\" type=\"text/x-svelte\">\n<p>a</p>\n</artifact>"
            ),
            "<p>a</p>\n"
        );
        assert_eq!(
            extract_code("Here:\n```tsx\nexport const A = 1;\n```\nDone"),
            "export const A = 1;\n"
        );
        assert_eq!(extract_code("plain"), "plain\n");
        assert_eq!(
            extract_code("<artifact type=\"text/tsx\">```tsx\nx\n```</artifact>"),
            "x\n"
        );
    }

    #[test]
    fn framework_names() {
        assert_eq!(
            Framework::parse("web-component"),
            Some(Framework::WebComponent)
        );
        assert_eq!(Framework::parse("angular"), None);
    }
}
