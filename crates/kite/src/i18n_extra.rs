//! Chinese for the layout strings. Kept beside [`super::ZH`] so `i18n.rs` stays under the
//! line limit.

pub(super) const PAIRS: &[(&str, &str)] = &[
    ("{} panes", "{} 窗格"),
    ("Could not open “{}”", "无法打开「{}」"),
    ("Group name", "分组名称"),
    ("Layout", "布局"),
    ("Layout name", "布局名称"),
    ("Layouts", "布局"),
    ("Move to {}", "移到 {}"),
    ("Name is required.", "名称不能为空。"),
    ("New group", "新建分组"),
    ("New group…", "新建分组…"),
    ("No session to save", "没有可保存的会话"),
    ("Open", "打开"),
    ("Remove from group", "移出分组"),
    ("Remove group", "移除分组"),
    ("Removed layout “{}”", "已移除布局「{}」"),
    ("Rename group", "重命名分组"),
    ("Rename group…", "重命名分组…"),
    ("Rename layout", "重命名布局"),
    ("Renamed layout to “{}”", "布局已重命名为「{}」"),
    ("Save as new layout…", "另存为新布局…"),
    ("Save layout", "保存布局"),
    ("Save layout…", "保存布局…"),
    ("Saved layout “{}”", "已保存布局「{}」"),
    ("Skipped missing hosts: {}", "已跳过缺失的主机：{}"),
    ("Update layout", "更新布局"),
    ("Updated layout “{}”", "已更新布局「{}」"),
    (
        "layouts.json could not be read; it was not overwritten",
        "无法读取 layouts.json，未覆盖该文件",
    ),
];

#[cfg(test)]
mod tests {
    use super::PAIRS;

    #[test]
    fn every_layout_string_has_chinese() {
        for (en, zh) in PAIRS {
            assert_ne!(en, zh, "{en}");
            assert_eq!(en.matches("{}").count(), zh.matches("{}").count(), "{en}");
            assert_eq!(super::super::lookup(true, en), *zh, "{en}");
        }
    }
}
