#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::Path;

use super::*;
use crate::split::{RATIO_MAX, RATIO_MIN};

fn sample() -> SavedTabs {
    SavedTabs {
        version: CURRENT,
        tabs: vec![
            tab("prod", Some("prod db"), 0, leaf("prod")),
            tab("staging", None, 0, leaf("staging")),
            tab("root@10.0.0.2:2222", None, 0, leaf("root@10.0.0.2:2222")),
        ],
        active: 1,
    }
}

fn tab(alias: &str, title: Option<&str>, focused: usize, tree: SavedNode) -> SavedTab {
    SavedTab {
        kind: SavedKind::Ssh {
            alias: alias.into(),
        },
        title: title.map(str::to_owned),
        tree: Some(tree),
        focused,
    }
}

fn leaf(alias: &str) -> SavedNode {
    SavedNode::leaf(alias)
}

fn split(axis: SavedAxis, ratio: f32, first: SavedNode, second: SavedNode) -> SavedNode {
    SavedNode::split(axis, ratio, first, second)
}

fn ssh(alias: &str) -> SavedTab {
    tab(alias, None, 0, leaf(alias))
}

fn ready(dir: &Path) -> SavedTabs {
    match SavedTabs::load(dir) {
        Load::Ready(saved) => saved,
        other => panic!("expected tabs, got {other:?}"),
    }
}

fn saved(active: usize, tabs: Vec<SavedTab>) -> SavedTabs {
    SavedTabs {
        version: CURRENT,
        tabs,
        active,
    }
}

#[test]
fn tabs_round_trip_with_order_titles_and_the_active_slot() {
    let dir = tempfile::tempdir().unwrap();
    sample().save(dir.path()).unwrap();
    assert_eq!(SavedTabs::load(dir.path()), Load::Ready(sample()));
}

#[test]
fn a_split_round_trips_with_its_ratio_focus_and_title() {
    let tab = tab(
        "staging",
        Some("ops"),
        1,
        split(SavedAxis::Row, 0.25, leaf("prod"), leaf("staging")),
    );
    let original = saved(0, vec![tab]);
    let dir = tempfile::tempdir().unwrap();
    original.save(dir.path()).unwrap();
    let loaded = ready(dir.path());
    assert_eq!(loaded, original);
    assert_eq!(loaded.sanitize(|_| true), original);
}

#[test]
fn a_same_host_split_round_trips_as_two_leaves() {
    let original = saved(
        0,
        vec![tab(
            "prod",
            None,
            1,
            split(SavedAxis::Column, 0.5, leaf("prod"), leaf("prod")),
        )],
    );
    let dir = tempfile::tempdir().unwrap();
    original.save(dir.path()).unwrap();
    let loaded = ready(dir.path());
    assert_eq!(loaded, original);
    let tree = loaded.tabs[0].tree.as_ref().unwrap();
    assert_eq!(tree.aliases(), ["prod", "prod"]);
}

#[test]
fn an_old_file_without_a_tree_loads_as_one_pane_and_saves_as_version_1() {
    let golden = r#"{"tabs":[{"kind":"ssh","alias":"prod","title":"prod db"}],"active":0}"#;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    std::fs::write(&path, golden).unwrap();
    let loaded = ready(dir.path());
    assert_eq!(
        loaded,
        saved(0, vec![tab("prod", Some("prod db"), 0, leaf("prod"))])
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), golden);

    loaded.save(dir.path()).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"version\": 1"), "{text}");
    assert!(text.contains("\"tree\""), "{text}");
    assert!(text.contains("\"alias\": \"prod\""), "{text}");
    assert_eq!(ready(dir.path()), loaded);

    // An unknown field is ignored, on the file and on a leaf.
    let extra = r#"{"version":1,"extra":true,"tabs":[{"kind":"ssh","alias":"prod","cwd":"/tmp","tree":{"alias":"prod","later":1}}],"active":0}"#;
    std::fs::write(&path, extra).unwrap();
    assert_eq!(ready(dir.path()).tabs, vec![ssh("prod")]);
}

#[test]
fn an_older_version_is_migrated_in_memory_and_not_rewritten() {
    let original = r#"{"version":0,"tabs":[{"kind":"ssh","alias":"prod"}],"active":0}"#;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    std::fs::write(&path, original).unwrap();
    let loaded = ready(dir.path());
    assert_eq!(loaded.version, CURRENT);
    assert_eq!(loaded.tabs, vec![ssh("prod")]);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn invalid_ratios_axes_and_branches_degrade() {
    let keep = saved(
        0,
        vec![tab(
            "b",
            None,
            9,
            split(SavedAxis::Unknown, f32::NAN, leaf("a"), leaf("b")),
        )],
    );
    let cleaned = keep.sanitize(|_| true);
    assert_eq!(
        cleaned.tabs,
        vec![tab(
            "b",
            None,
            1,
            split(SavedAxis::Row, even_ratio(), leaf("a"), leaf("b")),
        )]
    );

    for ratio in [f32::INFINITY, f32::NEG_INFINITY] {
        let cleaned = saved(
            0,
            vec![tab(
                "a",
                None,
                0,
                split(SavedAxis::Column, ratio, leaf("a"), leaf("b")),
            )],
        )
        .sanitize(|_| true);
        assert_eq!(
            cleaned.tabs[0].tree.as_ref().unwrap().ratio,
            Some(even_ratio())
        );
    }

    let low = saved(
        0,
        vec![tab(
            "a",
            None,
            0,
            split(SavedAxis::Row, 0.01, leaf("a"), leaf("b")),
        )],
    )
    .sanitize(|_| true);
    assert_eq!(low.tabs[0].tree.as_ref().unwrap().ratio, Some(RATIO_MIN));

    let high = saved(
        0,
        vec![tab(
            "b",
            None,
            1,
            split(SavedAxis::Column, 0.99, leaf("a"), leaf("b")),
        )],
    )
    .sanitize(|_| true);
    assert_eq!(high.tabs[0].tree.as_ref().unwrap().ratio, Some(RATIO_MAX));

    let one_sided = SavedNode {
        alias: None,
        axis: Some(SavedAxis::Column),
        ratio: Some(0.99),
        first: None,
        second: Some(Box::new(leaf("only"))),
    };
    let collapsed = saved(0, vec![tab("only", None, 3, one_sided)]).sanitize(|_| true);
    assert_eq!(collapsed.tabs, vec![ssh("only")]);

    let empty = SavedNode {
        alias: None,
        axis: Some(SavedAxis::Row),
        ratio: Some(0.4),
        first: Some(Box::new(SavedNode {
            alias: None,
            axis: None,
            ratio: None,
            first: None,
            second: None,
        })),
        second: None,
    };
    let gone = saved(0, vec![tab("x", None, 0, empty)]).sanitize(|_| true);
    assert!(gone.tabs.is_empty());
    assert_eq!(gone.active_slot(), 0);

    let raw = r#"{
        "version": 1,
        "active": 0,
        "tabs": [{
            "kind": "ssh",
            "alias": "a",
            "focused": 4,
            "tree": {
                "axis": "diagonal",
                "ratio": 0.01,
                "first": { "alias": "a" },
                "second": { "alias": "b" }
            }
        }]
    }"#;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    std::fs::write(&path, raw).unwrap();
    let cleaned = ready(dir.path()).sanitize(|_| true);
    assert_eq!(
        cleaned.tabs,
        vec![tab(
            "b",
            None,
            1,
            split(SavedAxis::Row, RATIO_MIN, leaf("a"), leaf("b")),
        )]
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
}

#[test]
fn unknown_hosts_are_dropped_and_the_tree_collapses() {
    let keep = |alias: &str| alias == "a" || alias == "b";
    let nested = split(
        SavedAxis::Row,
        0.25,
        leaf("a"),
        split(SavedAxis::Column, 0.5, leaf("gone"), leaf("b")),
    );
    // Focus was the dropped middle leaf, so the leaf before it takes the keyboard.
    let cleaned = saved(0, vec![tab("gone", None, 1, nested)]).sanitize(keep);
    assert_eq!(
        cleaned.tabs,
        vec![tab(
            "a",
            None,
            0,
            split(SavedAxis::Row, 0.25, leaf("a"), leaf("b")),
        )]
    );

    let first_gone = split(SavedAxis::Row, 0.5, leaf("gone"), leaf("a"));
    let cleaned = saved(0, vec![tab("gone", None, 0, first_gone)]).sanitize(keep);
    assert_eq!(cleaned.tabs, vec![ssh("a")]);

    let focus_past_the_end = split(SavedAxis::Row, 0.5, leaf("a"), leaf("gone"));
    let cleaned = saved(0, vec![tab("a", None, 9, focus_past_the_end)]).sanitize(keep);
    assert_eq!(cleaned.tabs, vec![ssh("a")]);

    let file = saved(
        2,
        vec![tab("gone", None, 0, leaf("gone")), ssh("a"), ssh("b")],
    );
    let cleaned = file.sanitize(keep);
    assert_eq!(cleaned.tabs, vec![ssh("a"), ssh("b")]);
    assert_eq!(cleaned.active, 1);

    let middle = saved(
        1,
        vec![
            ssh("a"),
            tab("gone", Some("skip"), 0, leaf("nope")),
            ssh("b"),
        ],
    );
    let cleaned = middle.sanitize(keep);
    assert_eq!(cleaned.tabs, vec![ssh("a"), ssh("b")]);
    assert_eq!(cleaned.active, 1);

    let tail = saved(
        2,
        vec![ssh("a"), ssh("b"), tab("gone", None, 0, leaf("nope"))],
    );
    let cleaned = tail.sanitize(keep);
    assert_eq!(cleaned.tabs, vec![ssh("a"), ssh("b")]);
    assert_eq!(cleaned.active, 2);
    assert_eq!(cleaned.active_slot(), 1);
}

#[test]
fn a_file_with_old_local_tabs_loads_and_skips_them() {
    let old = |active: usize| {
        format!(
            r#"{{"tabs":[{{"kind":"ssh","alias":"a"}},{{"kind":"local","title":"mine"}},{{"kind":"ssh","alias":"b"}}],"active":{active}}}"#
        )
    };
    for (active, expect) in [(0, 0), (1, 1), (2, 1)] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), old(active)).unwrap();
        let loaded = ready(dir.path());
        assert_eq!(loaded.tabs, [ssh("a"), ssh("b")]);
        assert_eq!(loaded.active, expect, "active {active}");
    }
}

#[test]
fn a_missing_file_is_no_tabs_and_may_be_written() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(SavedTabs::load(dir.path()), Load::Missing);
    assert!(SavedTabs::load(dir.path()).may_write());
    assert!(sample().save_if_writable(dir.path()).unwrap());
    assert_eq!(SavedTabs::load(dir.path()), Load::Ready(sample()));
}

#[test]
fn a_corrupt_file_is_ignored_and_left_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    std::fs::write(&path, "{ not json").unwrap();
    assert_eq!(SavedTabs::load(dir.path()), Load::Unreadable);
    assert!(!SavedTabs::load(dir.path()).may_write());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    assert!(!sample().save_if_writable(dir.path()).unwrap());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
}

#[test]
fn a_newer_version_is_refused_and_left_as_it_was() {
    let text = "{\n  \"version\": 2,\n  \"tabs\": [{\"kind\": \"ssh\", \"alias\": \"prod\"}],\n  \"active\": 0\n}";
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    std::fs::write(&path, text).unwrap();
    assert_eq!(SavedTabs::load(dir.path()), Load::Unreadable);
    assert!(!SavedTabs::load(dir.path()).may_write());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    assert!(!sample().save_if_writable(dir.path()).unwrap());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
}

#[test]
fn the_file_holds_names_only() {
    let dir = tempfile::tempdir().unwrap();
    sample().save(dir.path()).unwrap();
    let text = std::fs::read_to_string(dir.path().join(FILE_NAME)).unwrap();
    assert!(text.contains("\"kind\": \"ssh\""), "{text}");
    assert!(text.contains("\"version\": 1"), "{text}");
    assert!(!text.contains("unknown"), "{text}");
    for secret in ["password", "passphrase", "identity", "key"] {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
}

#[test]
fn a_stale_active_slot_is_held_inside_the_list() {
    let mut saved = sample();
    saved.active = 9;
    assert_eq!(saved.active_slot(), 2);
    assert_eq!(SavedTabs::default().active_slot(), 0);
}
