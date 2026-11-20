#![allow(clippy::unwrap_used, clippy::panic)]

use super::*;

fn leaf(alias: &str) -> SavedNode {
    SavedNode::leaf(alias)
}

fn sample_tree() -> SavedNode {
    SavedNode::split(
        SavedAxis::Row,
        0.3,
        leaf("prod"),
        SavedNode::split(SavedAxis::Column, 0.5, leaf("gone"), leaf("staging")),
    )
}

fn book_with(id: &str, name: &str) -> SavedLayouts {
    let mut book = SavedLayouts::default();
    book.layouts.push(SavedLayout {
        id: id.into(),
        name: name.into(),
        tree: leaf("prod"),
        focused: 0,
        title: None,
    });
    book
}

#[test]
fn a_missing_file_is_missing_and_a_round_trip_keeps_the_tree() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(SavedLayouts::load(dir.path()), Load::Missing);
    assert!(SavedLayouts::load(dir.path()).may_write());

    let mut book = SavedLayouts::default();
    let id = book
        .insert_new("生产监控", sample_tree(), 2, Some("ops".into()))
        .unwrap();
    book.save(dir.path()).unwrap();
    assert!(!dir.path().join("layouts.json.tmp").exists());

    let Load::Ready(loaded) = SavedLayouts::load(dir.path()) else {
        panic!("expected layouts");
    };
    assert_eq!(loaded.version, CURRENT);
    assert_eq!(loaded.layouts.len(), 1);
    assert_eq!(loaded.layouts[0].id, id);
    assert_eq!(loaded.layouts[0].name, "生产监控");
    assert_eq!(loaded.layouts[0].focused, 2);
    assert_eq!(loaded.layouts[0].title.as_deref(), Some("ops"));
    assert_eq!(
        loaded.layouts[0].tree.aliases(),
        ["prod", "gone", "staging"]
    );
    assert_eq!(loaded.layouts[0].pane_count(), 3);
}

#[test]
fn two_new_layouts_never_share_an_id() {
    let mut book = SavedLayouts::default();
    let first = book.insert_new("one", leaf("a"), 0, None).unwrap();
    let second = book.insert_new("two", leaf("b"), 9, None).unwrap();
    assert_ne!(first, second);
    assert_eq!(book.layouts[1].focused, 0, "focus clamps to the last pane");
    assert!(book.insert_new("  ", leaf("a"), 0, None).is_none());
    assert!(book.insert_new("named", empty_node(), 0, None).is_none());
    assert_eq!(book.layouts.len(), 2);
}

#[test]
fn rename_update_and_remove_follow_the_id() {
    let mut book = book_with("lay-1", "old");
    assert!(book.rename("lay-1", " 生产监控 "));
    assert_eq!(book.layouts[0].name, "生产监控");
    assert!(!book.rename("lay-1", "  "));
    assert!(!book.rename("missing", "other"));
    assert_eq!(book.layouts[0].name, "生产监控");

    assert!(book.update("lay-1", sample_tree(), 2, Some("ops".into())));
    assert_eq!(book.layouts[0].tree.aliases(), ["prod", "gone", "staging"]);
    assert_eq!(book.layouts[0].focused, 2);
    assert_eq!(book.layouts[0].title.as_deref(), Some("ops"));
    assert_eq!(book.layouts[0].name, "生产监控");
    assert!(!book.update("missing", leaf("a"), 0, None));
    assert!(!book.update("lay-1", empty_node(), 0, None));

    assert!(book.remove("lay-1"));
    assert!(book.layouts.is_empty());
    assert!(!book.remove("lay-1"));
}

#[test]
fn a_corrupt_or_newer_file_is_left_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("layouts.json");
    std::fs::write(&path, "{ not json").unwrap();
    assert_eq!(SavedLayouts::load(dir.path()), Load::Unreadable);
    assert!(!SavedLayouts::load(dir.path()).may_write());
    assert!(book_with("a", "n").save(dir.path()).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");

    let newer = format!(
        r#"{{"version":{},"layouts":[{{"id":"keep","name":"n","tree":{{"alias":"prod"}},"focused":0}}]}}"#,
        CURRENT + 1
    );
    std::fs::write(&path, &newer).unwrap();
    assert_eq!(SavedLayouts::load(dir.path()), Load::Unreadable);
    assert!(book_with("a", "n").save(dir.path()).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), newer);
}

#[test]
fn a_stray_tmp_beside_a_good_file_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    book_with("lay-1", "prod").save(dir.path()).unwrap();
    let good = std::fs::read_to_string(dir.path().join("layouts.json")).unwrap();
    std::fs::write(dir.path().join("layouts.json.tmp"), "{ half writ").unwrap();
    let Load::Ready(loaded) = SavedLayouts::load(dir.path()) else {
        panic!("tmp must not hide the real file");
    };
    assert_eq!(loaded.layouts[0].id, "lay-1");
    book_with("lay-2", "next").save(dir.path()).unwrap();
    let text = std::fs::read_to_string(dir.path().join("layouts.json")).unwrap();
    assert!(text.contains("lay-2"));
    assert_ne!(text, good);
}

#[test]
fn a_tmp_without_the_real_file_is_not_loaded() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("layouts.json.tmp"),
        r#"{"version":1,"layouts":[{"id":"ghost","name":"g","tree":{"alias":"prod"},"focused":0}]}"#,
    )
    .unwrap();
    assert_eq!(SavedLayouts::load(dir.path()), Load::Missing);
}

#[test]
fn pruning_drops_rejected_leaves_and_keeps_the_rest() {
    let opened = prune(&sample_tree(), 2, |alias| alias != "gone").unwrap();
    assert_eq!(opened.missing, ["gone"]);
    assert_eq!(opened.tree.aliases(), ["prod", "staging"]);
    assert_eq!(opened.focused, 1, "staging stays focused");
    assert_eq!(opened.tree.ratio, Some(0.3));

    let moved = prune(&sample_tree(), 1, |alias| alias != "gone").unwrap();
    assert_eq!(
        moved.focused, 0,
        "a dropped focus moves to the pane before it"
    );

    let err = prune(&sample_tree(), 0, |_| false).unwrap_err();
    assert_eq!(err, ["prod", "gone", "staging"]);
    assert!(prune(&empty_node(), 0, |_| true).is_err());
}

#[test]
fn a_missing_name_opens_only_when_it_parses_or_is_local() {
    let known = |alias: &str| alias == "prod";
    assert!(alias_openable("prod", known));
    assert!(alias_openable("Local Terminal", known));
    assert!(alias_openable("本地终端", known));
    assert!(alias_openable("root@10.0.0.2:2222", known));
    assert!(!alias_openable("host:0", known));
    assert!(!alias_openable("", known));
    assert!(!is_local_alias("prod"));
}

#[test]
fn search_text_includes_the_name_and_every_leaf() {
    let layout = SavedLayout {
        id: "a".into(),
        name: "生产监控".into(),
        tree: sample_tree(),
        focused: 0,
        title: None,
    };
    let text = search_text(&layout);
    assert!(text.contains("生产监控"));
    assert!(text.contains("prod") && text.contains("staging") && text.contains("gone"));
}

fn empty_node() -> SavedNode {
    SavedNode {
        alias: None,
        cwd: None,
        axis: None,
        ratio: None,
        first: None,
        second: None,
    }
}

#[test]
fn a_layout_does_not_keep_a_local_directory() {
    let mut book = SavedLayouts::default();
    let tree = SavedNode::with_cwd("Local Terminal", Some("/tmp/only-here".into()));
    assert!(book.insert_new("desk", tree, 0, None).is_some());
    assert_eq!(book.layouts[0].tree.cwd, None);

    let id = book.layouts[0].id.clone();
    let nested = SavedNode::split(
        SavedAxis::Row,
        0.5,
        SavedNode::with_cwd("Local Terminal", Some("/tmp/a".into())),
        leaf("prod"),
    );
    assert!(book.update(&id, nested, 0, None));
    let tree = &book.layouts[0].tree;
    assert_eq!(tree.cwd, None);
    assert_eq!(tree.first.as_ref().and_then(|node| node.cwd.clone()), None);
}
