#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::{choose_shell_pid, parse_lsof_cwd, parse_processes, restored_directory};

#[test]
fn lsof_keeps_paths_with_spaces_and_skips_a_pid_with_no_path() {
    let text = "\
p10
fcwd
n/Users/me/My Documents
p11
fcwd
p12
n/tmp/项目
";
    let map = parse_lsof_cwd(text);
    assert_eq!(
        map.get(&10).map(|path| path.as_os_str()),
        Some(OsStr::new("/Users/me/My Documents"))
    );
    assert!(!map.contains_key(&11));
    assert_eq!(map.get(&12).unwrap(), Path::new("/tmp/项目"));
}

#[test]
fn empty_lsof_output_and_a_path_before_any_pid_add_nothing() {
    assert!(parse_lsof_cwd("").is_empty());
    assert!(parse_lsof_cwd("\n\n").is_empty());
    let map = parse_lsof_cwd("n/tmp\np3\nnot-a-field\nn/ok\n");
    assert_eq!(map.len(), 1);
    assert_eq!(map.get(&3).unwrap(), Path::new("/ok"));
}

#[test]
fn a_missing_process_is_simply_absent() {
    let map = parse_lsof_cwd("p1\nfcwd\nn/tmp\n");
    assert!(map.contains_key(&1));
    assert!(!map.contains_key(&2));
    assert!(!map.contains_key(&0));
}

#[test]
fn login_resolves_to_its_shell_and_a_shell_does_not_resolve_to_its_command() {
    let text = "\
10 1 /usr/bin/login
11 10 helper
12 10 -/bin/zsh
13 12 vim
20 1 -zsh
";
    let procs = parse_processes(text);
    assert_eq!(choose_shell_pid(10, &procs), 12);
    assert_eq!(choose_shell_pid(12, &procs), 12);
    assert_eq!(choose_shell_pid(20, &procs), 20);
    assert_eq!(choose_shell_pid(99, &procs), 99);
}

#[test]
fn a_saved_directory_is_used_only_when_it_is_still_a_directory() {
    assert!(restored_directory(None).is_none());
    assert!(restored_directory(Some("")).is_none());
    assert!(restored_directory(Some("   ")).is_none());
    assert!(restored_directory(Some("/no/such/kite-cwd")).is_none());

    let file = std::env::temp_dir().join(format!("kite-not-a-dir-{}", std::process::id()));
    std::fs::write(&file, b"x").unwrap();
    let text = file.to_str().unwrap();
    assert!(restored_directory(Some(text)).is_none());
    std::fs::remove_file(&file).unwrap();

    let dir = std::env::temp_dir();
    let text = dir.to_str().unwrap();
    assert_eq!(
        restored_directory(Some(text)).as_deref(),
        Some(dir.as_path())
    );
}

#[cfg(unix)]
#[test]
fn a_cd_in_the_shell_is_what_a_later_read_returns() {
    let root = std::env::temp_dir().join(format!("kite-cwd-{}", std::process::id()));
    let start = root.join("start");
    let next = root.join("next");
    std::fs::create_dir_all(&start).unwrap();
    std::fs::create_dir_all(&next).unwrap();
    let pty = kite_term::LocalPty::spawn_in(80, 24, Some(&start)).unwrap();
    let pid = pty.child_pid().expect("pty child");
    // A login shell writes its startup text. If nobody reads the PTY, the shell
    // blocks before it can run `cd`.
    let reader = pty.reader().unwrap();
    let _drain = std::thread::spawn(move || {
        use std::io::Read as _;
        let mut reader = reader;
        let mut buf = [0_u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(_) => {}
                // The master fd is non-blocking. WouldBlock is "no bytes yet", not EOF.
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.kind() == std::io::ErrorKind::Interrupted =>
                {
                    // This thread only drains the PTY. Sleeping here does not block the UI.
                    #[allow(clippy::disallowed_methods)]
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let quoted = shell_quote(next.to_str().unwrap());
    let mut saw_next = false;
    for _ in 0..40 {
        // Interactive shells accept a line on CR. A bare LF never leaves the line editor.
        let _ = pty.write(format!("cd {quoted}\r").as_bytes());
        if saw_dir(&rt, pid, &next) {
            saw_next = true;
            break;
        }
        rt.block_on(async {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        });
    }
    drop(pty);
    let _ = std::fs::remove_dir_all(&root);
    assert!(saw_next, "shell cwd did not follow cd");
}

#[cfg(unix)]
fn saw_dir(rt: &tokio::runtime::Runtime, pid: u32, want: &Path) -> bool {
    let found = rt.block_on(super::query_cwds(&[pid]));
    same_dir(found.get(&pid).map(PathBuf::as_path), want)
}

#[cfg(unix)]
fn same_dir(got: Option<&Path>, want: &Path) -> bool {
    let Some(got) = got else {
        return false;
    };
    match (std::fs::canonicalize(got), std::fs::canonicalize(want)) {
        (Ok(got), Ok(want)) => got == want,
        _ => got == want,
    }
}

#[cfg(unix)]
fn shell_quote(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}
