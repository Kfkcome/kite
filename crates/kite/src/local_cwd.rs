//! Where a local shell is, without asking the user to edit `.zshrc` or `.bashrc`.
//!
//! macOS PTYs are `login` processes. `login -l` does not follow `cd`, so the directory
//! is taken from the shell that `login` started. Linux reads `/proc/<pid>/cwd`.
//! A missing path, or one that is no longer a directory, is not a directory to restore.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// How long a quit-time read may take. GPUI only waits 200ms on the future returned
/// from `on_app_quit`, and it drops windows before polling that future, so the read
/// itself runs inside the callback.
const QUERY_TIMEOUT: Duration = Duration::from_millis(500);
const QUERY_WAIT: Duration = Duration::from_millis(550);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Process {
    pid: u32,
    ppid: u32,
    comm: String,
}

/// A saved directory that still exists. `None`, blank, and anything that is not a
/// directory mean the shell should start in `$HOME` instead.
#[must_use]
pub(crate) fn restored_directory(saved: Option<&str>) -> Option<PathBuf> {
    let saved = saved.map(str::trim).filter(|dir| !dir.is_empty())?;
    let path = PathBuf::from(saved);
    path.is_dir().then_some(path)
}

/// Reads `pids` (PTY children) asynchronously on `handle` without blocking the caller's executor.
pub(crate) async fn query(handle: &tokio::runtime::Handle, pids: &[u32]) -> HashMap<u32, PathBuf> {
    if pids.is_empty() {
        return HashMap::new();
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    let pids = pids.to_vec();
    handle.spawn(async move {
        let found = query_with_timeout(pids).await;
        let _ = tx.send(found);
    });
    rx.await.unwrap_or_default()
}

/// Reads `pids` (PTY children) without blocking the caller past [`QUERY_WAIT`].
/// The map is keyed by those pids, not by the shell pid underneath `login`.
pub(crate) fn query_blocking(
    handle: &tokio::runtime::Handle,
    pids: &[u32],
) -> HashMap<u32, PathBuf> {
    if pids.is_empty() {
        return HashMap::new();
    }
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let pids = pids.to_vec();
    handle.spawn(async move {
        let found = query_with_timeout(pids).await;
        let _ = tx.send(found);
    });
    rx.recv_timeout(QUERY_WAIT).unwrap_or_default()
}

async fn query_with_timeout(pids: Vec<u32>) -> HashMap<u32, PathBuf> {
    tokio::time::timeout(QUERY_TIMEOUT, query_cwds(&pids))
        .await
        .unwrap_or_default()
}

async fn query_cwds(roots: &[u32]) -> HashMap<u32, PathBuf> {
    if roots.is_empty() {
        return HashMap::new();
    }
    #[cfg(target_os = "macos")]
    {
        macos_cwds(roots).await
    }
    #[cfg(target_os = "linux")]
    {
        linux_cwds(roots)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        HashMap::new()
    }
}

#[cfg(target_os = "linux")]
fn linux_cwds(roots: &[u32]) -> HashMap<u32, PathBuf> {
    let mut out = HashMap::new();
    for &pid in roots {
        let Ok(path) = std::fs::read_link(format!("/proc/{pid}/cwd")) else {
            continue;
        };
        if path.is_dir() {
            out.insert(pid, path);
        }
    }
    out
}

#[cfg(target_os = "macos")]
async fn macos_cwds(roots: &[u32]) -> HashMap<u32, PathBuf> {
    let Some(procs) = read_processes().await else {
        return HashMap::new();
    };
    let mut shell_of_root = HashMap::new();
    for &root in roots {
        shell_of_root.insert(choose_shell_pid(root, &procs), root);
    }
    let shells: Vec<u32> = shell_of_root.keys().copied().collect();
    let mut out = HashMap::new();
    for (shell, path) in lsof_cwds(&shells).await {
        if path.is_dir()
            && let Some(root) = shell_of_root.get(&shell)
        {
            out.insert(*root, path);
        }
    }
    out
}

#[cfg(target_os = "macos")]
async fn read_processes() -> Option<Vec<Process>> {
    let mut cmd = tokio::process::Command::new("/bin/ps");
    cmd.args(["-ax", "-o", "pid=,ppid=,comm="])
        .kill_on_drop(true);
    let out = cmd.output().await.ok()?;
    if out.stdout.is_empty() {
        return None;
    }
    Some(parse_processes(&String::from_utf8_lossy(&out.stdout)))
}

#[cfg(target_os = "macos")]
async fn lsof_cwds(pids: &[u32]) -> HashMap<u32, PathBuf> {
    if pids.is_empty() {
        return HashMap::new();
    }
    let list = pids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let mut cmd = tokio::process::Command::new("/usr/sbin/lsof");
    // `-b -w` keeps `lsof` from blocking on stalled mounts; `-n -P` skips host/port lookups.
    cmd.args([
        "-b", "-w", "-n", "-P", "-a", "-d", "cwd", "-Fn", "-p", &list,
    ])
    .kill_on_drop(true);
    match cmd.output().await {
        Ok(out) => parse_lsof_cwd(&String::from_utf8_lossy(&out.stdout)),
        Err(error) => {
            tracing::warn!(error = %error, "local_cwd_lsof_failed");
            HashMap::new()
        }
    }
}

/// `lsof -F` fields: `p<pid>`, `fcwd`, `n<path>`. A pid with no `n` line is absent.
fn parse_lsof_cwd(stdout: &str) -> HashMap<u32, PathBuf> {
    let mut out = HashMap::new();
    let mut pid = None;
    for line in stdout.lines() {
        let Some(rest) = line.get(1..) else {
            continue;
        };
        match line.as_bytes().first().copied() {
            Some(b'p') => pid = rest.parse().ok(),
            Some(b'n') if !rest.is_empty() => {
                if let Some(pid) = pid {
                    out.insert(pid, PathBuf::from(rest));
                }
            }
            _ => {}
        }
    }
    out
}

fn parse_processes(stdout: &str) -> Vec<Process> {
    stdout.lines().filter_map(parse_process_line).collect()
}

fn parse_process_line(line: &str) -> Option<Process> {
    let mut parts = line.split_whitespace();
    let pid = parts.next()?.parse().ok()?;
    let ppid = parts.next()?.parse().ok()?;
    let comm = parts.next().unwrap_or("").to_owned();
    Some(Process { pid, ppid, comm })
}

/// The shell under a macOS `login` child. A pid that is already a shell is kept,
/// so a foreground command (a grandchild) is not mistaken for the shell.
fn choose_shell_pid(root: u32, procs: &[Process]) -> u32 {
    let root_is_shell = procs
        .iter()
        .find(|proc| proc.pid == root)
        .is_some_and(|proc| is_shell(&proc.comm));
    if root_is_shell {
        return root;
    }
    let mut kids: Vec<&Process> = procs.iter().filter(|proc| proc.ppid == root).collect();
    kids.sort_by_key(|proc| proc.pid);
    if let Some(shell) = kids.iter().find(|proc| is_shell(&proc.comm)) {
        return shell.pid;
    }
    kids.first().map(|proc| proc.pid).unwrap_or(root)
}

fn is_shell(comm: &str) -> bool {
    let comm = comm.trim().trim_start_matches('-');
    let base = comm.rsplit('/').next().unwrap_or(comm);
    matches!(
        base,
        "zsh" | "bash" | "sh" | "fish" | "dash" | "ksh" | "tcsh" | "csh"
    )
}

#[cfg(test)]
#[path = "local_cwd_tests.rs"]
mod tests;
