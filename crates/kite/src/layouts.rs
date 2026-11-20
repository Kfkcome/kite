//! Saved workspace layouts in `layouts.json`: a name, the split tree, and which pane is focused.
//! Names and layout only: no secret, no command, no terminal contents.
//!
//! A missing file is [`Load::Missing`]. A newer `version`, or a file that does not parse, is
//! [`Load::Unreadable`] and is never replaced.

use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::split::clamp_ratio;
use crate::tabs_store::{SavedAxis, SavedNode, focus_among};

const FILE_NAME: &str = "layouts.json";

/// The `layouts.json` this kite writes.
pub const CURRENT: u32 = 1;

/// Collapse key for the sidebar's Layouts section, stored with the host groups.
pub const GROUP: &str = "__layouts__";

fn current_version() -> u32 {
    CURRENT
}

/// One saved arrangement of panes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedLayout {
    pub id: String,
    pub name: String,
    /// Split tree. Each leaf `alias` is a host alias, `user@host:port`, or a local terminal.
    pub tree: SavedNode,
    /// Focused leaf, in [`SavedNode::aliases`] order.
    pub focused: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl SavedLayout {
    /// How many panes the tree opens.
    pub fn pane_count(&self) -> usize {
        self.tree.aliases().len()
    }
}

/// What the picker and the sidebar match a layout against: its name and every leaf alias.
pub fn search_text(layout: &SavedLayout) -> String {
    let mut text = layout.name.clone();
    for alias in layout.tree.aliases() {
        text.push(' ');
        text.push_str(&alias);
    }
    text
}

/// The alias a local terminal pane is stored as, in either interface language.
pub fn is_local_alias(alias: &str) -> bool {
    alias == "Local Terminal" || alias == "本地终端"
}

/// A leaf can open when it is a saved host, a local terminal, or a target [`ConnectSpec`] parses.
///
/// [`ConnectSpec`]: kite_ssh::ConnectSpec
pub fn alias_openable(alias: &str, known: impl Fn(&str) -> bool) -> bool {
    is_local_alias(alias) || known(alias) || kite_ssh::ConnectSpec::parse(alias).is_ok()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedLayouts {
    #[serde(default = "current_version")]
    pub version: u32,
    #[serde(default)]
    pub layouts: Vec<SavedLayout>,
}

impl Default for SavedLayouts {
    fn default() -> Self {
        Self {
            version: CURRENT,
            layouts: Vec::new(),
        }
    }
}

/// What reading `layouts.json` produced. [`Load::Unreadable`] must not be written over.
#[derive(Debug, Clone, PartialEq)]
pub enum Load {
    /// No file yet. The next save may create it.
    Missing,
    /// This version could read it.
    Ready(SavedLayouts),
    /// Newer than this kite, or not JSON this version understands.
    Unreadable,
}

impl Load {
    /// A missing or readable file may be replaced. A newer or corrupt one may not.
    pub fn may_write(&self) -> bool {
        !matches!(self, Load::Unreadable)
    }
}

/// A layout after leaves `keep` rejected were removed.
#[derive(Debug, Clone, PartialEq)]
pub struct Opened {
    pub tree: SavedNode,
    pub focused: usize,
    /// Aliases that were dropped.
    pub missing: Vec<String>,
}

/// Drops leaves `keep` rejects and points `focused` at a pane that remains.
///
/// # Errors
///
/// The missing aliases, when the tree is empty or every pane was rejected.
pub fn prune(
    tree: &SavedNode,
    focused: usize,
    keep: impl Fn(&str) -> bool,
) -> Result<Opened, Vec<String>> {
    let Some(tree) = repair(tree.clone()) else {
        return Err(Vec::new());
    };
    let aliases = tree.aliases();
    let kept: Vec<bool> = aliases.iter().map(|alias| keep(alias)).collect();
    let missing = aliases
        .iter()
        .zip(&kept)
        .filter(|(_, on)| !**on)
        .map(|(alias, _)| alias.clone())
        .collect::<Vec<_>>();
    let Some(focused) = focus_among(focused, &kept) else {
        return Err(missing);
    };
    let mut cursor = 0;
    let Some(tree) = drop_unkept(&tree, &kept, &mut cursor) else {
        return Err(missing);
    };
    Ok(Opened {
        tree,
        focused,
        missing,
    })
}

impl SavedLayouts {
    /// The saved layouts. A missing file is [`Load::Missing`]. A newer version, or a file that
    /// does not parse, is [`Load::Unreadable`] and is not modified.
    pub fn load(dir: &Path) -> Load {
        let text = match std::fs::read_to_string(dir.join(FILE_NAME)) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Load::Missing,
            Err(e) => {
                tracing::warn!(error = %e, "layouts_json_unreadable");
                return Load::Unreadable;
            }
        };
        let saved = match serde_json::from_str::<SavedLayouts>(&text) {
            Ok(saved) => saved,
            Err(e) => {
                tracing::warn!(error = %e, "layouts_json_unreadable");
                return Load::Unreadable;
            }
        };
        if saved.version > CURRENT {
            tracing::warn!(version = saved.version, "layouts_json_newer");
            return Load::Unreadable;
        }
        Load::Ready(saved)
    }

    /// Temp file + rename. Refuses when the file on disk is newer or unreadable, and leaves
    /// that file as it was. Always writes [`CURRENT`].
    ///
    /// # Errors
    ///
    /// When the directory or file cannot be written, or the file on disk must not be replaced.
    pub fn save(&self, dir: &Path) -> io::Result<()> {
        if !Self::load(dir).may_write() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "layouts.json is newer or unreadable",
            ));
        }
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

    /// Appends a layout with a new id. `None` when the name is blank or the tree has no pane.
    pub fn insert_new(
        &mut self,
        name: &str,
        tree: SavedNode,
        focused: usize,
        title: Option<String>,
    ) -> Option<String> {
        let name = name.trim();
        let tree = tree.without_cwd();
        let count = tree.aliases().len();
        if name.is_empty() || count == 0 {
            return None;
        }
        let id = fresh_id(&self.layouts);
        self.layouts.push(SavedLayout {
            id: id.clone(),
            name: name.to_owned(),
            tree,
            focused: focused.min(count - 1),
            title,
        });
        Some(id)
    }

    /// Replaces the tree, focus and title of `id`. The name stays.
    pub fn update(
        &mut self,
        id: &str,
        tree: SavedNode,
        focused: usize,
        title: Option<String>,
    ) -> bool {
        let tree = tree.without_cwd();
        let count = tree.aliases().len();
        if count == 0 {
            return false;
        }
        let Some(layout) = self.layouts.iter_mut().find(|layout| layout.id == id) else {
            return false;
        };
        layout.tree = tree;
        layout.focused = focused.min(count - 1);
        layout.title = title;
        true
    }

    /// Sets the display name. Blank names and unknown ids are left as they were.
    pub fn rename(&mut self, id: &str, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let Some(layout) = self.layouts.iter_mut().find(|layout| layout.id == id) else {
            return false;
        };
        layout.name = name.to_owned();
        true
    }

    /// Drops the layout with `id`.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.layouts.len();
        self.layouts.retain(|layout| layout.id != id);
        self.layouts.len() != before
    }
}

fn fresh_id(existing: &[SavedLayout]) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut id = format!("{tick:x}{n:x}");
    let mut salt = 0u32;
    while existing.iter().any(|layout| layout.id == id) {
        salt = salt.wrapping_add(1);
        id = format!("{tick:x}{n:x}{salt:x}");
        if salt == 0 {
            break;
        }
    }
    id
}

fn repair(node: SavedNode) -> Option<SavedNode> {
    if !node.is_split() {
        return node.alias.map(SavedNode::leaf);
    }
    let first = node.first.and_then(|child| repair(*child));
    let second = node.second.and_then(|child| repair(*child));
    let axis = match node.axis {
        Some(SavedAxis::Column) => SavedAxis::Column,
        _ => SavedAxis::Row,
    };
    let ratio = clamp_ratio(node.ratio.unwrap_or(0.5));
    match (first, second) {
        (Some(first), Some(second)) => Some(SavedNode::split(axis, ratio, first, second)),
        (Some(only), None) | (None, Some(only)) => Some(only),
        (None, None) => None,
    }
}

fn drop_unkept(node: &SavedNode, kept: &[bool], cursor: &mut usize) -> Option<SavedNode> {
    if node.is_split() {
        let axis = node.axis.unwrap_or(SavedAxis::Row);
        let ratio = node.ratio.unwrap_or(0.5);
        let first = node
            .first
            .as_deref()
            .and_then(|child| drop_unkept(child, kept, cursor));
        let second = node
            .second
            .as_deref()
            .and_then(|child| drop_unkept(child, kept, cursor));
        return match (first, second) {
            (Some(first), Some(second)) => Some(SavedNode::split(axis, ratio, first, second)),
            (Some(only), None) | (None, Some(only)) => Some(only),
            (None, None) => None,
        };
    }
    let alias = node.alias.clone()?;
    let keep = kept.get(*cursor).copied().unwrap_or(false);
    *cursor += 1;
    keep.then_some(SavedNode::leaf(alias))
}

#[cfg(test)]
#[path = "layouts_tests.rs"]
mod tests;
