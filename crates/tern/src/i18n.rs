//! Interface language. English strings are the keys; a missing Chinese entry stays English.
//! Logs and host data are not translated.

use std::sync::atomic::{AtomicU8, Ordering};

use crate::settings::Language;

/// 0 is English, 1 is Chinese. Set from settings before the window draws.
static LANG: AtomicU8 = AtomicU8::new(0);

/// Applies `language` and reports whether the interface language changed.
pub fn install(language: Language) -> bool {
    let on = u8::from(language.is_chinese(crate::platform::system_prefers_chinese()));
    LANG.swap(on, Ordering::Relaxed) != on
}

/// The English source string, or its Chinese text when that language is on.
pub fn tr(en: &'static str) -> &'static str {
    lookup(LANG.load(Ordering::Relaxed) == 1, en)
}

/// Fills `{}` holes in `en` (or its translation) from left to right.
pub fn fill(en: &'static str, parts: &[&str]) -> String {
    let mut rest = tr(en);
    let mut out = String::new();
    for part in parts {
        match rest.split_once("{}") {
            Some((head, tail)) => {
                out.push_str(head);
                out.push_str(part);
                rest = tail;
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

fn lookup(chinese: bool, en: &'static str) -> &'static str {
    if !chinese {
        return en;
    }
    ZH.iter()
        .find(|(key, _)| *key == en)
        .map(|(_, text)| *text)
        .unwrap_or(en)
}

const ZH: &[(&str, &str)] = &[
    ("+ Add", "＋ 添加"),
    ("{} disconnected", "{} 已断开"),
    ("{} fills the password", "{} 会填入密码"),
    (
        "{} · fills the window; the empty view shows it sharp",
        "{} · 铺满窗口；空白页显示清晰原图",
    ),
    ("{} is already “{}”", "{} 已经是「{}」"),
    (
        "{} is kept for quitting and the clipboard",
        "{} 保留给退出和剪贴板",
    ),
    (
        "{} opens the picker in a session. {}",
        "{} 可在会话里打开片段列表。{}",
    ),
    (
        "{} saved in tern. Import copies hosts out of ~/.ssh/config once; it is never read or written afterwards.",
        "tern 里保存了 {} 个。导入会把 ~/.ssh/config 里的主机复制一次；之后不再读取或写入该文件。",
    ),
    ("{} saved secret", "已保存 {} 个机密"),
    ("{} saved secrets", "已保存 {} 个机密"),
    (
        "-L 8080:db:5432   (-R and -D too)",
        "-L 8080:db:5432（也支持 -R 和 -D）",
    ),
    (
        "4 to 8 digits, kept in the Keychain on this Mac. The vault must be unlocked to set it.",
        "4 到 8 位数字，保存在这台 Mac 的钥匙串里。设置时保险箱必须是解锁的。",
    ),
    (
        "A key named \"{}\" already exists.",
        "已有名为「{}」的密钥。",
    ),
    (
        "A new ed25519 key, stored only in the vault",
        "一把新的 ed25519 密钥，只存在保险箱里",
    ),
    ("About", "关于"),
    ("Account", "账户"),
    ("Add", "添加"),
    ("Add in Settings", "在设置里添加"),
    ("Advanced", "高级"),
    ("Already in tern", "已在 tern 中"),
    ("Already up to date", "已经是最新"),
    (
        "An OpenSSH private key without its own passphrase",
        "没有单独口令的 OpenSSH 私钥",
    ),
    ("App", "应用"),
    ("Appearance", "外观"),
    (
        "At launch and 5 seconds after a change to connections, settings, shortcuts or the vault",
        "启动时，以及连接、设置、快捷键或保险箱变更 5 秒后",
    ),
    (
        "Automatic sync is waiting for the vault to be unlocked",
        "自动同步正在等待保险箱解锁",
    ),
    ("Back", "返回"),
    ("Bar", "竖线"),
    ("BCAST", "广播"),
    ("Bell", "响铃"),
    ("Bigger text", "放大文字"),
    ("Blink", "闪烁"),
    ("Block", "方块"),
    ("Bounce the Dock icon", "弹跳程序坞图标"),
    ("Bounce the Dock icon on bell", "响铃时弹跳程序坞图标"),
    ("Broadcast input to…", "把输入广播到…"),
    ("Browse files", "浏览文件"),
    ("Cancel", "取消"),
    ("Change", "更改"),
    ("Change passphrase", "更改口令"),
    (
        "Changed on this Mac and on the remote",
        "这台 Mac 和远程都改过",
    ),
    ("Check for updates", "检查更新"),
    (
        "Choose a passphrase. It cannot be recovered if you forget it.",
        "设一个口令。忘记了无法找回。",
    ),
    ("Choose an image…", "选择图片…"),
    ("Choose wallpaper", "选择壁纸"),
    (
        "Click a shortcut, then press the new keys. Esc cancels, Backspace unbinds. Keys without Ctrl or Alt always go to the remote shell.",
        "点一下快捷键，再按下新的按键。Esc 取消，Backspace 解除绑定。没有 Ctrl 或 Alt 的按键总会发给远程 shell。",
    ),
    (
        "Click a shortcut, then press the new keys. Esc cancels, ⌫ unbinds. Keys without ⌘, ⌃ or ⌥ always go to the remote shell.",
        "点一下快捷键，再按下新的按键。Esc 取消，⌫ 解除绑定。没有 ⌘、⌃ 或 ⌥ 的按键总会发给远程 shell。",
    ),
    ("Close", "关闭"),
    ("Close other tabs", "关闭其他标签"),
    ("Close tab", "关闭标签"),
    ("closed", "已关闭"),
    ("closed by server", "服务器关闭了连接"),
    ("Command", "命令"),
    ("Connect", "连接"),
    ("Connect to a host…", "连接到主机…"),
    ("Connected", "已连接"),
    ("Connecting…", "正在连接…"),
    ("connection reset", "连接被重置"),
    (
        "Connection lost ({}). Reconnecting in {}s\u{2026} (Enter to retry now, Esc to stop)",
        "连接已断开（{}）。{} 秒后重连…（Enter 立即重试，Esc 停止）",
    ),
    ("Connections", "连接"),
    (
        "Connections come back waiting for Enter, so none dials until you ask",
        "连接会恢复并等待 Enter，不会自己拨号",
    ),
    (
        "Connections, settings, shortcuts and the vault, encrypted before they leave",
        "连接、设置、快捷键和保险箱，离开这台 Mac 之前会加密",
    ),
    (
        "Copies the checked hosts into tern once. The file is never read again or changed.",
        "把勾选的主机复制进 tern，只此一次。之后不再读取或修改该文件。",
    ),
    ("Copy", "复制"),
    ("Copy on select", "选中即复制"),
    ("Copy public key", "复制公钥"),
    ("Could not change it: {}", "无法更改：{}"),
    ("Could not create it: {}", "无法创建：{}"),
    ("Could not save keymap.json: {}", "无法保存 keymap.json：{}"),
    ("Could not save the vault: {}", "无法保存保险箱：{}"),
    ("Could not start the log: {}", "无法开始记录：{}"),
    ("Create", "创建"),
    (
        "Create a private repo on GitHub",
        "在 GitHub 上创建私有仓库",
    ),
    ("Create the vault", "创建保险箱"),
    (
        "Created the first time you save a password, or here",
        "第一次保存密码时创建，也可以在这里创建",
    ),
    ("Creating…", "正在创建…"),
    ("Credential Manager", "凭据管理器"),
    ("Current passphrase", "当前口令"),
    ("Cursor", "光标"),
    ("Cursor blink", "光标闪烁"),
    ("Dark", "深色"),
    ("Default terminal theme", "默认终端主题"),
    ("Default text size", "默认字号"),
    ("Delete", "删除"),
    ("Delete?", "删除？"),
    ("Disconnected", "已断开"),
    ("Dither", "抖动"),
    ("Download", "下载"),
    ("Downloaded from the remote", "已从远程下载"),
    ("Downloaded the remote's changes", "已下载远程的更改"),
    ("Edit", "编辑"),
    ("Edit connection", "编辑连接"),
    ("Edit snippet", "编辑片段"),
    ("Edit…", "编辑…"),
    ("Effect", "效果"),
    (
        "Empty: a private GitHub gist is used instead",
        "留空则使用私有 GitHub gist",
    ),
    ("Enter a new passphrase.", "请输入新口令。"),
    ("Enter the current passphrase.", "请输入当前口令。"),
    (
        "Enter the path of a private key file.",
        "请输入私钥文件的路径。",
    ),
    ("Enter the vault PIN", "输入保险箱 PIN"),
    ("Enter the vault passphrase", "输入保险箱口令"),
    (
        "Enter the vault passphrase first: it encrypts what goes to GitHub",
        "请先输入保险箱口令：发往 GitHub 的内容用它加密",
    ),
    ("File not found", "找不到文件"),
    ("Fill password", "填入密码"),
    ("Finishing a selection copies it", "选区完成时复制内容"),
    ("Fixed", "固定"),
    ("Flash the terminal", "闪烁终端"),
    ("Flash the terminal on bell", "响铃时闪烁终端"),
    ("Focus pane down", "聚焦下方窗格"),
    ("Focus pane left", "聚焦左侧窗格"),
    ("Focus pane right", "聚焦右侧窗格"),
    ("Focus pane up", "聚焦上方窗格"),
    ("Font", "字体"),
    ("Font size", "字号"),
    (
        "For every host without its own; set one per host from the sidebar",
        "用于没有单独主题的主机；可在侧边栏为每台主机单独设置",
    ),
    ("Forget token", "忘记令牌"),
    ("Forward SSH agent", "转发 SSH agent"),
    ("General", "通用"),
    ("Generate", "生成"),
    ("ghp_… with the gist scope", "ghp_…，只需 gist 权限"),
    ("Git remote", "Git 远程"),
    ("GitHub", "GitHub"),
    ("GitHub token removed", "已移除 GitHub 令牌"),
    ("GitHub token saved", "已保存 GitHub 令牌"),
    ("Give the key a name first.", "请先给密钥起个名字。"),
    ("Group", "分组"),
    ("Halftone", "半调"),
    ("Hide Others", "隐藏其他"),
    ("Hide tern", "隐藏 tern"),
    (
        "Hold the connecting pulse and sidebar slide still. On by itself when macOS Reduce motion is on",
        "连接时的呼吸和侧边栏滑动保持静止。macOS 的「减弱动态效果」打开时也会自动生效",
    ),
    ("Host", "主机"),
    ("Host actions", "主机操作"),
    (
        "Host, connection state and session time under the terminal",
        "终端下方显示主机、连接状态和会话时间",
    ),
    ("Hosts", "主机"),
    (
        "Hosts from ~/.ssh/config are listed in the sidebar",
        "~/.ssh/config 里的主机会出现在侧边栏",
    ),
    ("How strongly the picture shows", "图片显示的强弱"),
    (
        "How the key is listed, and how a connection picks it",
        "密钥在列表里的名字，连接也用它来选用",
    ),
    ("Image", "图片"),
    ("Import", "导入"),
    ("Import {}", "导入 {}"),
    ("Import a key file", "导入密钥文件"),
    ("Import from ~/.ssh/config", "从 ~/.ssh/config 导入"),
    ("Import from ~/.ssh/config…", "从 ~/.ssh/config 导入…"),
    ("Imported {} host", "已导入 {} 台主机"),
    ("Imported {} hosts", "已导入 {} 台主机"),
    ("Insert a snippet…", "插入片段…"),
    ("Jump host", "跳板机"),
    ("Keep this Mac", "保留这台 Mac"),
    (
        "Keep this Mac uploads it over the remote's; Use remote replaces this Mac's",
        "「保留这台 Mac」会覆盖远程；「使用远程」会替换这台 Mac 的内容",
    ),
    ("Keep-alive (seconds, 0 = off)", "保活（秒，0 为关闭）"),
    ("Key file", "密钥文件"),
    ("Key name", "密钥名称"),
    ("Key name, e.g. laptop", "密钥名称，例如 laptop"),
    ("Key passphrase", "密钥口令"),
    ("Key {} saved", "密钥 {} 已保存"),
    ("Keychain", "钥匙串"),
    ("Keyboard and mouse", "键盘和鼠标"),
    ("Language", "语言"),
    ("Last sync failed: {}", "上次同步失败：{}"),
    ("Light", "浅色"),
    ("lines", "行"),
    (
        "Lines kept per terminal; Ctrl+Shift+F searches them",
        "每个终端保留的行数；Ctrl+Shift+F 可搜索",
    ),
    (
        "Lines kept per terminal; ⌘F searches them",
        "每个终端保留的行数；⌘F 可搜索",
    ),
    ("Lock now", "立即锁定"),
    ("Lock vault after idle", "空闲后锁定保险箱"),
    ("Locked", "已锁定"),
    ("LOG", "记录"),
    ("Log every session", "记录每个会话"),
    ("Log saved to {}", "日志已保存到 {}"),
    ("Logging", "日志记录"),
    ("Logging to {}", "正在记录到 {}"),
    ("Logs", "日志"),
    ("Merged this Mac with the remote", "已把这台 Mac 和远程合并"),
    ("Merged with the remote", "已与远程合并"),
    ("Middle-click pastes", "中键粘贴"),
    ("min", "分钟"),
    ("Minimize", "最小化"),
    (
        "Monospace fonts on this Mac; a Nerd Font \"Mono\" keeps icons one cell wide",
        "这台 Mac 上的等宽字体；Nerd Font 的 Mono 版本能让图标占满一格",
    ),
    ("Motion", "动效"),
    ("Name", "名称"),
    (
        "Name of a key in the vault (optional)",
        "保险箱里的密钥名称（可选）",
    ),
    ("Navigate", "移动"),
    ("New connection", "新建连接"),
    ("New passphrase", "新口令"),
    ("New PIN, 4 to 8 digits", "新 PIN，4 到 8 位"),
    ("New snippet", "新建片段"),
    ("New tab to this host", "用此主机开新标签"),
    ("New tab: find a host", "新标签：查找主机"),
    (
        "New tabs write plain text to {}/sessions; or start one from a tab's menu",
        "新标签会把纯文本写到 {}/sessions；也可以从标签菜单单独开始",
    ),
    ("New vault passphrase", "新的保险箱口令"),
    ("Next / previous tab", "下一个 / 上一个标签"),
    ("Next tab", "下一个标签"),
    (
        "No Downloads folder to save into.",
        "没有可保存的“下载”文件夹。",
    ),
    ("No connections yet", "还没有连接"),
    ("No group", "无分组"),
    (
        "No hosts found in ~/.ssh/config",
        "在 ~/.ssh/config 里没有找到主机",
    ),
    ("No hosts match", "没有匹配的主机"),
    (
        "No hosts match. Enter connects to \"{}\".",
        "没有匹配的主机。按 Enter 将连接到「{}」。",
    ),
    ("No hosts yet.", "还没有主机。"),
    (
        "No key or mouse input to the window. Open sessions keep running.",
        "窗口没有键盘或鼠标输入。已打开的会话继续运行。",
    ),
    ("No snippets match", "没有匹配的片段"),
    ("No snippets yet.", "还没有片段。"),
    (
        "No snippets yet. Save a command you type often.",
        "还没有片段。把常输入的命令存下来。",
    ),
    (
        "No vault here yet: GitHub's passphrase, or a new one to start",
        "这台 Mac 还没有保险箱：用 GitHub 上的口令，或设一个新的来开始",
    ),
    ("No vault yet", "还没有保险箱"),
    (
        "No vault yet: this creates one, locked with this passphrase.",
        "还没有保险箱：这将用此口令创建一个。",
    ),
    ("No wallpaper", "无壁纸"),
    ("None", "无"),
    ("Not connected", "未连接"),
    ("Not connected · Enter connects", "未连接 · 按 Enter 连接"),
    ("Nothing saved yet", "还没有保存内容"),
    ("Off", "关闭"),
    (
        "On 1x monitors: whole-pixel sizes, thinner icons, an opaque window and stronger muted text",
        "在 1x 显示器上：对齐到整像素、更细的图标、不透明窗口，以及更深的次要文字",
    ),
    (
        "On. Opens the vault after an idle lock; 5 wrong PINs turn it off. Changing the passphrase removes it.",
        "已开启。空闲锁定后用它打开保险箱；连续 5 次错误会关闭。更改口令会移除它。",
    ),
    (
        "Once a day, ask GitHub for a newer release; tern never installs one",
        "每天向 GitHub 询问一次是否有新版本；tern 不会自动安装",
    ),
    (
        "One private gist on your GitHub account. GitHub stores only encrypted data.",
        "你的 GitHub 账户上的一个私有 gist。GitHub 只存加密后的数据。",
    ),
    (
        "Open a session to use a snippet",
        "先打开一个会话才能使用片段",
    ),
    ("Open host picker", "打开主机选择"),
    ("Open in new tab", "在新标签中打开"),
    ("Open settings", "打开设置"),
    (
        "Optional when gh is signed in. Needs only the gist scope.",
        "gh 已登录时可以不填。只需要 gist 权限。",
    ),
    ("Optional, saved to the vault", "可选，保存到保险箱"),
    ("Output", "输出"),
    (
        "Panels carry the text, sized to the picture; off keeps it to the empty view",
        "面板承载文字，并随图片调整；关闭时只出现在空白页",
    ),
    ("Password", "密码"),
    ("Password saved in the vault", "密码已存入保险箱"),
    ("Password command", "密码命令"),
    (
        "Passwords, key passphrases and SSH keys, encrypted with your vault passphrase (age, scrypt)",
        "密码、密钥口令和 SSH 密钥，用保险箱口令加密（age、scrypt）",
    ),
    ("Personal access token", "个人访问令牌"),
    ("Port", "端口"),
    ("Port forward {} failed: {}", "端口转发 {} 失败：{}"),
    ("Port forwards", "端口转发"),
    ("Port forwards…", "端口转发…"),
    ("Press keys…", "请按键…"),
    ("Previous tab", "上一个标签"),
    (
        "Prints the password, e.g. gopass show -o …",
        "打印密码，例如 gopass show -o …",
    ),
    ("Private repo ready: {}", "私有仓库已就绪：{}"),
    ("Public key copied", "已复制公钥"),
    ("Public key of {}", "{} 的公钥"),
    ("Quit", "退出"),
    ("Quit tern", "退出 tern"),
    ("Quit, hide, minimise", "退出、隐藏、最小化"),
    ("Recent", "最近"),
    ("Reconnect", "重新连接"),
    ("Reconnect current session", "重新连接当前会话"),
    ("Reduce motion", "减弱动态效果"),
    ("Remove", "移除"),
    ("Removed {}", "已移除 {}"),
    ("Removed {} from the vault", "已从保险箱移除 {}"),
    ("Removed from the Keychain", "已从钥匙串移除"),
    ("Rename…", "重命名…"),
    ("Rendered once and cached", "渲染一次后缓存"),
    ("Reopen tabs on launch", "启动时重新打开标签"),
    ("Repeat PIN", "再次输入 PIN"),
    ("Repeat new passphrase", "再次输入新口令"),
    ("Repeat passphrase", "再次输入口令"),
    ("Repeat vault passphrase", "再次输入保险箱口令"),
    ("Repository", "仓库"),
    ("Reset", "重置"),
    ("Resolve", "处理冲突"),
    (
        "Runs gh repo create tern-sync --private",
        "运行 gh repo create tern-sync --private",
    ),
    ("Save", "保存"),
    ("Saved {}", "已保存 {}"),
    ("Saved in tern", "保存在 tern 里"),
    (
        "Saved passwords and keys appear here",
        "保存的密码和密钥会显示在这里",
    ),
    ("Saved secrets", "已保存的机密"),
    ("Saved snippets", "已保存的片段"),
    ("Saving…", "正在保存…"),
    ("Scanlines", "扫描线"),
    ("Scrollback", "回滚"),
    ("Search hosts", "搜索主机"),
    ("Search {} themes…", "搜索 {} 个主题…"),
    ("Secret Service", "密钥环服务"),
    ("Security", "安全"),
    ("Security & sync", "安全与同步"),
    ("Set PIN", "设置 PIN"),
    ("Settings", "设置"),
    ("Settings sections", "设置分区"),
    ("Settings…", "设置…"),
    ("Shape", "形状"),
    (
        "Sharper text on standard displays",
        "在普通显示器上使用更清晰的文字",
    ),
    ("Shortcuts", "快捷键"),
    ("Show", "显示"),
    ("Show All", "全部显示"),
    ("Show status line", "显示状态栏"),
    ("Smaller text", "缩小文字"),
    ("Snippets", "片段"),
    ("Split down", "向下分屏"),
    ("Split right", "向右分屏"),
    ("SSH key", "SSH 密钥"),
    ("SSH keys", "SSH 密钥"),
    ("Start logging…", "开始记录…"),
    ("Startup", "启动"),
    ("Status", "状态"),
    ("Stop broadcasting", "停止广播"),
    ("Stop logging", "停止记录"),
    (
        "Stores the vault passphrase in your login Keychain and opens the vault the first time tern needs it",
        "把保险箱口令存在登录钥匙串里，tern 第一次需要时自动打开",
    ),
    ("Sudo password command", "Sudo 密码命令"),
    (
        "Surfaces, accent and hover washes lean toward the image's dominant colour",
        "表面、强调色和悬停色会偏向图片的主色",
    ),
    ("Switch to tab 1–9", "切换到标签 1–9"),
    ("Sync", "同步"),
    ("Sync automatically", "自动同步"),
    (
        "Sync needs your decision: open Settings, Sync",
        "同步需要你做选择：打开设置里的同步",
    ),
    ("Sync now", "立即同步"),
    (
        "Synced with your own git and its SSH keys",
        "使用你自己的 git 及其 SSH 密钥同步",
    ),
    ("Syncing through a GitHub gist", "通过 GitHub gist 同步"),
    (
        "Syncing through your git repository",
        "通过你的 git 仓库同步",
    ),
    ("Syncing…", "正在同步…"),
    ("System", "跟随系统"),
    (
        "System follows macOS; the default terminal colours follow it too",
        "跟随系统时使用 macOS 的外观；默认的终端颜色也会跟着变",
    ),
    (
        "System uses the language macOS lists first",
        "跟随系统时，使用 macOS 列出的第一种语言",
    ),
    ("Tags", "标签"),
    ("Terminal", "终端"),
    ("Terminal text", "终端文字"),
    ("Terminal theme", "终端主题"),
    (
        "That file is too large to be a key.",
        "这个文件太大，不像是密钥。",
    ),
    ("The current passphrase is wrong.", "当前口令不正确。"),
    (
        "The new passphrase and its confirmation differ.",
        "新口令和确认输入不一致。",
    ),
    (
        "The new passphrase is the same as the current one.",
        "新口令和当前口令相同。",
    ),
    (
        "The session is not connected, so nothing was typed",
        "会话未连接，所以没有输入",
    ),
    (
        "The vault is re-encrypted; the old passphrase stops working",
        "保险箱会重新加密；旧口令随即失效",
    ),
    (
        "The vault will open the first time tern needs it",
        "tern 第一次需要时会打开保险箱",
    ),
    ("Theme", "主题"),
    ("Theme colours from wallpaper", "从壁纸取主题色"),
    ("Theme for {}", "{} 的主题"),
    ("Theme…", "主题…"),
    ("There is no vault yet.", "还没有保险箱。"),
    ("timeout", "超时"),
    ("Toggle sidebar", "切换侧边栏"),
    (
        "Token saved in the macOS Keychain",
        "令牌已保存在 macOS 钥匙串",
    ),
    ("Turn off", "关闭"),
    ("Type it", "输入"),
    ("Unchanged", "保持不变"),
    ("Underline", "下划线"),
    ("Undo", "撤销"),
    ("Unlock", "解锁"),
    ("Unlock the vault first.", "请先解锁保险箱。"),
    ("Unlock with a PIN", "用 PIN 解锁"),
    ("Unlock with the {}", "通过{}解锁"),
    ("Unlocked", "已解锁"),
    ("Unlocking…", "正在解锁…"),
    (
        "Unlocks the first time a saved login is needed, or here",
        "第一次需要已保存的登录信息时解锁，也可以在这里解锁",
    ),
    (
        "Unlocks this Mac's vault for the sync",
        "为同步解锁这台 Mac 的保险箱",
    ),
    ("unreachable", "无法到达"),
    ("Up to date", "已是最新"),
    ("Uploaded this Mac's changes", "已上传这台 Mac 的更改"),
    ("Uploaded to the remote", "已上传到远程"),
    ("Use Option as Meta", "将 Option 用作 Meta"),
    ("Use PIN instead", "改用 PIN"),
    ("Use passphrase instead", "改用口令"),
    ("Use remote", "使用远程"),
    ("Use the default theme", "使用默认主题"),
    ("User", "用户"),
    (
        "Using the GitHub CLI's sign-in (gh)",
        "使用 GitHub CLI 的登录（gh）",
    ),
    ("Vault", "保险箱"),
    ("Vault PIN removed", "已移除保险箱 PIN"),
    ("Vault PIN set", "已设置保险箱 PIN"),
    ("Vault created", "保险箱已创建"),
    ("Vault locked", "保险箱已锁定"),
    (
        "Vault locked after {} minutes idle",
        "空闲 {} 分钟后，保险箱已锁定",
    ),
    ("Vault unavailable: {}", "保险箱不可用：{}"),
    ("Vault unlocked", "保险箱已解锁"),
    ("Vault: {}", "保险箱：{}"),
    ("Vault PIN", "保险箱 PIN"),
    ("Vault key", "保险箱密钥"),
    ("Vault passphrase", "保险箱口令"),
    ("Vault passphrase changed", "保险箱口令已更改"),
    (
        "Vault passphrase changed. The PIN was removed: set it again",
        "保险箱口令已更改。PIN 已移除，请重新设置",
    ),
    ("Version", "版本"),
    ("Visibility", "可见度"),
    ("Waiting for your answer", "等待你的回答"),
    ("Waiting for your choice", "等待你的选择"),
    ("Wallpaper", "壁纸"),
    ("Wallpaper fills the window", "壁纸铺满窗口"),
    ("When tern is in the background", "tern 在后台时"),
    ("Window", "窗口"),
    ("Working…", "正在处理…"),
    ("Wrong vault passphrase.", "保险箱口令不正确。"),
    (
        "Wrong vault passphrase. The connection is saved; the password is not.",
        "保险箱口令不正确。连接已保存，密码没有保存。",
    ),
    (
        "Your own private git repository. It stores only encrypted data.",
        "你自己的私有 git 仓库。里面只存加密后的数据。",
    ),
    ("Zoom", "缩放"),
    (
        "⌥B, ⌥F and friends reach the shell; off types the macOS character",
        "⌥B、⌥F 等会发给 shell；关闭时输入 macOS 的字符",
    ),
    (
        "↵ runs when picked; the rest are typed for you to finish.",
        "选中后 ↵ 会执行；其余内容会先输入，留给你补完。",
    ),
    (
        "↑ ↓ preview · ↵ keep · Esc revert",
        "↑ ↓ 预览 · ↵ 保留 · Esc 还原",
    ),
    ("~/.ssh/id_ed25519 (optional)", "~/.ssh/id_ed25519（可选）"),
    (
        "bastion, or user@host:port (optional)",
        "跳板机，或 user@host:port（可选）",
    ),
    ("comma separated, e.g. eu, db", "用逗号分隔，例如 eu, db"),
    ("e.g. gopass show -o …", "例如 gopass show -o …"),
    ("tern {} is available", "tern {} 已发布"),
    ("{}: {}", "{}：{}"),
    ("Authentication", "认证方式"),
    ("Browse…", "浏览…"),
    ("Choose JSON file…", "选择 JSON 文件…"),
    ("Choose Netcatty JSON export", "选择 Netcatty 导出的 JSON 文件"),
    ("Choose SSH private key", "选择 SSH 私钥文件"),
    (
        "Choose a Netcatty JSON export file to preview hosts",
        "请选择一个 Netcatty 导出的 JSON 文件以预览主机",
    ),
    ("Could not read Netcatty JSON", "无法读取 Netcatty JSON"),
    (
        "Enter a passphrase for the new vault.",
        "请输入新保险箱的口令。",
    ),
    ("Enter the vault PIN.", "请输入保险箱 PIN。"),
    ("Enter the vault passphrase.", "请输入保险箱口令。"),
    (
        "Enter the vault passphrase to save the password.",
        "请输入保险箱口令以保存密码。",
    ),
    ("Import from Netcatty JSON", "从 Netcatty JSON 导入"),
    ("Import from Netcatty JSON…", "从 Netcatty JSON 导入…"),
    (
        "No SSH hosts found in this Netcatty JSON file",
        "这个 Netcatty JSON 文件里没有找到 SSH 主机",
    ),
    (
        "No hosts array found in this Netcatty JSON file.",
        "这个 Netcatty JSON 文件里没有找到 hosts 数组。",
    ),
    (
        "Remember vault passphrase in Keychain",
        "在钥匙串中记住保险箱口令",
    ),
    (
        "The passphrase and its confirmation differ.",
        "两次输入的口令不一致。",
    ),
    (
        "The vault is locked: unlock it, then save the entry again",
        "保险箱已锁定：请先解锁，再重新保存该条目",
    ),
    ("The vault passphrases differ.", "两次输入的保险箱口令不一致。"),
    (
        "This is an encrypted Netcatty backup. In Netcatty, use Settings → Data → Export Vault → Sanitized Export (JSON).",
        "这是加密的 Netcatty 备份文件。请在 Netcatty 中使用「设置 → 数据 → 导出保险箱 → 脱敏导出 (JSON)」。",
    ),
    ("Local Terminal", "本地终端"),
    ("Open a local shell", "打开本地终端"),
    ("None running", "当前无运行中的转发"),
    ("Not connected.", "未连接。"),
    ("Not connected: no files to browse.", "未连接：无法浏览文件。"),
    ("Port forwards on {}", "{} 的端口转发"),
    ("Stop", "停止"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_and_placeholders_match() {
        let mut seen = std::collections::BTreeSet::new();
        for (en, zh) in ZH {
            assert!(seen.insert(*en), "duplicate {en}");
            assert_eq!(en.matches("{}").count(), zh.matches("{}").count(), "{en}");
            assert!(!zh.is_empty(), "{en}");
        }
    }

    #[test]
    fn chinese_replaces_a_known_label_and_keeps_an_unknown_one() {
        assert_eq!(lookup(true, "Appearance"), "外观");
        assert_eq!(lookup(false, "Appearance"), "Appearance");
        assert_eq!(lookup(true, "not a label"), "not a label");
    }

    #[test]
    fn fill_writes_parts_into_the_holes() {
        let got = {
            let mut rest = "a {} b {}";
            let mut out = String::new();
            for part in ["one", "two"] {
                let Some((head, tail)) = rest.split_once("{}") else {
                    break;
                };
                out.push_str(head);
                out.push_str(part);
                rest = tail;
            }
            out.push_str(rest);
            out
        };
        assert_eq!(got, "a one b two");
    }
}
