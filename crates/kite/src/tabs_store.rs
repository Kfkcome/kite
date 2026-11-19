//! The open tabs, kept in `tabs.json` next to the settings so the next launch can reopen them.
//! Names and layout only: no secret, no command, no terminal contents.
//!
//! ```text
//! { "version": 1, "active": 0, "tabs": [{
//!     "kind": "ssh", "alias": "staging", "title": "ops", "focused": 1,
//!     "tree": { "axis": "row", "ratio": 0.25,
//!       "first": { "alias": "prod" }, "second": { "alias": "staging" } }
//! }] }
//! ```
//!
//! `version` is written as 1. A missing version is 1, so a file from before splits still loads
//! as one pane per tab. `alias` is the focused leaf, which is all an older kite reads. `tree`
//! is optional; without it the tab is that one pane. Leaves are aliases, and the same host may
//! appear twice. `focused` indexes those leaves from left to right, then top to bottom. Every
//! split stores `axis` (`row` or `column`) and `ratio` (the share given to `first`). Pane ids
//! are not stored. A newer version, or a file that does not parse, is left untouched.

use std::io;
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::split::clamp_ratio;

const FILE_NAME: &str = "tabs.json";

/// The `tabs.json` this kite writes. A missing version is this too.
pub const CURRENT: u32 = 1;

fn current_version() -> u32 {
    CURRENT
}

fn even_ratio() -> f32 {
    0.5
}

/// Which way a saved split divides its area. An unrecognised name is kept so it can degrade
/// to [`SavedAxis::Row`] instead of rejecting the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SavedAxis {
    /// First on the left, second on the right.
    #[default]
    Row,
    /// First on top, second below.
    Column,
    /// A name this kite does not know.
    Unknown,
}

impl Serialize for SavedAxis {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            SavedAxis::Column => "column",
            SavedAxis::Row | SavedAxis::Unknown => "row",
        })
    }
}

impl<'de> Deserialize<'de> for SavedAxis {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(match raw.as_str() {
            "column" => SavedAxis::Column,
            "row" => SavedAxis::Row,
            _ => SavedAxis::Unknown,
        })
    }
}

/// A pane, or a split of two panes. A leaf is `{ "alias" }` only. A split adds `axis`,
/// `ratio`, `first` and `second`. A missing side is absent, not null, and collapses on read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedNode {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<SavedAxis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratio: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first: Option<Box<SavedNode>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second: Option<Box<SavedNode>>,
}

impl SavedNode {
    /// One pane, opened as `alias`.
    pub fn leaf(alias: impl Into<String>) -> Self {
        Self {
            alias: Some(alias.into()),
            axis: None,
            ratio: None,
            first: None,
            second: None,
        }
    }

    /// `first` on the left or top, taking `ratio` of the space.
    pub fn split(axis: SavedAxis, ratio: f32, first: Self, second: Self) -> Self {
        Self {
            alias: None,
            axis: Some(axis),
            ratio: Some(ratio),
            first: Some(Box::new(first)),
            second: Some(Box::new(second)),
        }
    }

    /// A split has children. A leaf is an alias and nothing else.
    pub(crate) fn is_split(&self) -> bool {
        self.first.is_some() || self.second.is_some()
    }

    fn aliases(&self) -> Vec<String> {
        if self.is_split() {
            let mut all = self
                .first
                .as_ref()
                .map(|node| node.aliases())
                .unwrap_or_default();
            if let Some(second) = &self.second {
                all.extend(second.aliases());
            }
            return all;
        }
        self.alias.iter().cloned().collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SavedKind {
    /// A connection, by the alias or `user@host:port` it was opened with.
    Ssh { alias: String },
    /// A kind this version does not know, such as the local shell tabs older versions kept.
    /// It is dropped when the file is read and never written.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedTab {
    #[serde(flatten)]
    pub kind: SavedKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The split tree. Absent on a file from before splits: the tab is the one `alias`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree: Option<SavedNode>,
    /// Which leaf has the keyboard, in [`SavedNode::aliases`] order.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub focused: usize,
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedTabs {
    #[serde(default = "current_version")]
    pub version: u32,
    pub tabs: Vec<SavedTab>,
    pub active: usize,
}

impl Default for SavedTabs {
    fn default() -> Self {
        Self {
            version: CURRENT,
            tabs: Vec::new(),
            active: 0,
        }
    }
}

/// What reading `tabs.json` produced. [`Load::Unreadable`] must not be written over.
#[derive(Debug, Clone, PartialEq)]
pub enum Load {
    /// No file yet. The next save may create it.
    Missing,
    /// This version could read it.
    Ready(SavedTabs),
    /// Newer than this tern, or not JSON this version understands.
    Unreadable,
}

impl Load {
    /// A missing or readable file may be replaced. A newer or corrupt one may not.
    pub fn may_write(&self) -> bool {
        !matches!(self, Load::Unreadable)
    }
}

impl SavedTabs {
    /// The tabs of the last run. A missing file is [`Load::Missing`]. A newer version, or a
    /// file that does not parse, is [`Load::Unreadable`] and is not modified.
    pub fn load(dir: &Path) -> Load {
        let text = match std::fs::read_to_string(dir.join(FILE_NAME)) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Load::Missing,
            Err(e) => {
                tracing::warn!(error = %e, "tabs_json_unreadable");
                return Load::Unreadable;
            }
        };
        let saved = match serde_json::from_str::<SavedTabs>(&text) {
            Ok(saved) => saved,
            Err(e) => {
                tracing::warn!(error = %e, "tabs_json_unreadable");
                return Load::Unreadable;
            }
        };
        if saved.version > CURRENT {
            tracing::warn!(version = saved.version, "tabs_json_newer");
            return Load::Unreadable;
        }
        let version = saved.version;
        let saved = if version < CURRENT {
            migrate(saved, version)
        } else {
            saved
        };
        Load::Ready(saved.without_unknown().materialize())
    }

    /// Temp file + rename, so a crash mid-write never leaves half a file. Always writes
    /// [`CURRENT`], whatever `version` the value was built with.
    ///
    /// # Errors
    ///
    /// When the directory or file cannot be written.
    pub fn save(&self, dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(FILE_NAME);
        let tmp = path.with_extension("json.tmp");
        let mut writing = self.clone();
        writing.version = CURRENT;
        std::fs::write(
            &tmp,
            serde_json::to_string_pretty(&writing).map_err(io::Error::other)?,
        )?;
        std::fs::rename(&tmp, &path)
    }

    /// Writes unless the file on disk is newer or unreadable. A missing file is created.
    /// The shell guards with its own flag and calls [`Self::save`]; tests use this.
    ///
    /// # Errors
    ///
    /// When the directory or file cannot be written. A refused write is `Ok(false)`.
    #[cfg(test)]
    pub fn save_if_writable(&self, dir: &Path) -> io::Result<bool> {
        if !Self::load(dir).may_write() {
            return Ok(false);
        }
        self.save(dir)?;
        Ok(true)
    }

    /// Drops tabs of a kind this version cannot open. The active slot keeps pointing at the
    /// same tab, or at the one that took the dropped tab's place.
    fn without_unknown(mut self) -> Self {
        let mut tabs = Vec::with_capacity(self.tabs.len());
        let mut active = self.active;
        for (ix, tab) in self.tabs.into_iter().enumerate() {
            if tab.kind == SavedKind::Unknown {
                if ix < self.active {
                    active = active.saturating_sub(1);
                }
            } else {
                tabs.push(tab);
            }
        }
        self.tabs = tabs;
        self.active = active;
        self
    }

    /// A tab with no tree is the single pane its `alias` names.
    fn materialize(mut self) -> Self {
        for tab in &mut self.tabs {
            if tab.tree.is_none()
                && let SavedKind::Ssh { alias } = &tab.kind
            {
                tab.tree = Some(SavedNode::leaf(alias.clone()));
            }
        }
        self
    }

    /// Repairs ratios, axes and empty branches, then drops leaves `keep` rejects. A tab with
    /// nothing left is removed. The file on disk is not touched.
    pub fn sanitize(self, keep: impl Fn(&str) -> bool) -> Self {
        let mut tabs = Vec::with_capacity(self.tabs.len());
        let mut active = self.active;
        for (ix, tab) in self.tabs.into_iter().enumerate() {
            match sanitize_tab(tab, &keep) {
                Some(tab) => tabs.push(tab),
                None if ix < self.active => active = active.saturating_sub(1),
                None => {}
            }
        }
        Self {
            version: self.version,
            tabs,
            active,
        }
    }

    /// The active slot, held to a tab that exists.
    pub fn active_slot(&self) -> usize {
        self.active.min(self.tabs.len().saturating_sub(1))
    }
}

/// Identity until a change is no longer additive. `from` is always below [`CURRENT`].
fn migrate(mut saved: SavedTabs, from: u32) -> SavedTabs {
    if from < CURRENT {
        saved.version = CURRENT;
    }
    saved
}

/// Where focus lands once `kept` says which leaves survive. An index past the end clamps to
/// the last leaf. A dropped leaf moves to the one before it, or the one after when it was first.
pub(crate) fn focus_among(focus: usize, kept: &[bool]) -> Option<usize> {
    if !kept.iter().any(|keep| *keep) {
        return None;
    }
    let focus = focus.min(kept.len() - 1);
    let chosen = if kept[focus] {
        focus
    } else {
        match (0..focus).rev().find(|&ix| kept[ix]) {
            Some(ix) => ix,
            None => (focus + 1..kept.len()).find(|&ix| kept[ix])?,
        }
    };
    Some(
        kept.iter()
            .copied()
            .take(chosen)
            .filter(|keep| *keep)
            .count(),
    )
}

fn sanitize_tab(tab: SavedTab, keep: &impl Fn(&str) -> bool) -> Option<SavedTab> {
    if matches!(tab.kind, SavedKind::Unknown) {
        return None;
    }
    let tree = tab.tree.or_else(|| match &tab.kind {
        SavedKind::Ssh { alias } => Some(SavedNode::leaf(alias.clone())),
        SavedKind::Unknown => None,
    })?;
    let tree = repair(tree)?;
    let kept: Vec<bool> = tree.aliases().iter().map(|alias| keep(alias)).collect();
    let focused = focus_among(tab.focused, &kept)?;
    let mut cursor = 0;
    let tree = drop_flags(tree, &kept, &mut cursor)?;
    let alias = tree.aliases().get(focused).cloned()?;
    Some(SavedTab {
        kind: SavedKind::Ssh { alias },
        title: tab.title,
        tree: Some(tree),
        focused,
    })
}

/// Clamps ratios, turns an unknown axis into a row, and collapses a split that lost a side.
fn repair(node: SavedNode) -> Option<SavedNode> {
    if !node.is_split() {
        return node.alias.map(SavedNode::leaf);
    }
    // A missing side collapses like `Node::remove`: the sibling takes the space.
    let first = node.first.and_then(|child| repair(*child));
    let second = node.second.and_then(|child| repair(*child));
    let axis = match node.axis {
        Some(SavedAxis::Column) => SavedAxis::Column,
        Some(SavedAxis::Row | SavedAxis::Unknown) | None => SavedAxis::Row,
    };
    let ratio = clamp_ratio(node.ratio.unwrap_or(even_ratio()));
    match (first, second) {
        (Some(first), Some(second)) => Some(SavedNode::split(axis, ratio, first, second)),
        (Some(only), None) | (None, Some(only)) => Some(only),
        (None, None) => None,
    }
}

fn drop_flags(node: SavedNode, kept: &[bool], cursor: &mut usize) -> Option<SavedNode> {
    if node.is_split() {
        let axis = node.axis.unwrap_or(SavedAxis::Row);
        let ratio = node.ratio.unwrap_or(even_ratio());
        let first = node
            .first
            .and_then(|child| drop_flags(*child, kept, cursor));
        let second = node
            .second
            .and_then(|child| drop_flags(*child, kept, cursor));
        return match (first, second) {
            (Some(first), Some(second)) => Some(SavedNode::split(axis, ratio, first, second)),
            (Some(only), None) | (None, Some(only)) => Some(only),
            (None, None) => None,
        };
    }
    let alias = node.alias?;
    let keep = kept.get(*cursor).copied().unwrap_or(false);
    *cursor += 1;
    keep.then_some(SavedNode::leaf(alias))
}

#[cfg(test)]
#[path = "tabs_store_tests.rs"]
mod tests;
