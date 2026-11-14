//! JSON shapes exchanged with the UI (`graph_page`, `graph_save_page`).

use serde::{Deserialize, Serialize};

use crate::inline::{self, Property, Ref};
use crate::parse::Block;

/// One block for the outliner. Only `content` and `children` are read back
/// on save; the other fields are derived from `content` for display.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockNode {
    pub content: String,
    #[serde(default)]
    pub children: Vec<BlockNode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduled: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<String>,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub properties: Vec<Property>,
    #[serde(default)]
    pub refs: Vec<Ref>,
}

impl BlockNode {
    /// Derive display fields from `content` (children left empty).
    #[must_use]
    pub fn from_content(content: &str) -> Self {
        let info = inline::analyze(content);
        Self {
            content: content.to_string(),
            children: Vec::new(),
            uuid: info.uuid(),
            collapsed: info.collapsed(),
            marker: info.marker,
            priority: info.priority,
            scheduled: info.scheduled,
            deadline: info.deadline,
            properties: info.properties,
            refs: info.refs,
        }
    }

    #[must_use]
    pub fn from_block(block: &Block) -> Self {
        let mut node = Self::from_content(&block.content);
        node.children = block.children.iter().map(Self::from_block).collect();
        node
    }

    /// Back to a parse-tree block (no source bytes; see
    /// [`crate::Document::replace_blocks`]).
    #[must_use]
    pub fn to_block(&self) -> Block {
        Block::new(self.content.replace("\r\n", "\n"))
            .with_children(self.children.iter().map(Self::to_block).collect())
    }
}

/// A page as the UI sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageView {
    pub name: String,
    /// Graph-relative file, `None` for a page that is only referenced.
    pub file: Option<String>,
    /// `yyyyMMdd` for journal pages.
    pub journal_day: Option<i64>,
    pub properties: Vec<Property>,
    /// Text before the first block (frontmatter, page properties), verbatim.
    pub preamble: String,
    pub blocks: Vec<BlockNode>,
}
