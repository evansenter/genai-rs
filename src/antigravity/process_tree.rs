//! Signals the harness *and* the processes it started.
//!
//! The harness runs in its own process group (spawned with
//! `process_group(0)`), so a signal to that group reaches it and everything
//! it started without moving them elsewhere, such as stdio MCP servers.
//! That alone is not enough: the harness gives every `run_command` command
//! a process group of its own (pgid == the command's pid on harness
//! 0.1.23), so the command and its children sit outside
//! the harness's group. Those groups are found through the harness's direct
//! children and signalled too.
//!
//! A command that leaves its group (`setsid`, a double-forked daemon) is out
//! of reach either way. The harness's own graceful exit (stdin EOF, the
//! [`HarnessProcess::shutdown`](super::process::HarnessProcess::shutdown)
//! path) stops its command groups itself; this module is for the forced
//! paths, where the harness gets no chance to.

/// Sends `signal` to the harness's process group and to the process groups
/// of the harness's direct children.
///
/// The children are listed *before* any signal is sent: once the harness
/// dies its children are reparented and can no longer be found through it.
/// Never signals this process's own group, or groups 0 and 1.
#[cfg(unix)]
pub(crate) fn signal_tree(harness_pid: u32, signal: libc::c_int) {
    let Ok(harness) = libc::pid_t::try_from(harness_pid) else {
        return;
    };
    let children = child_process_groups(harness);
    for group in target_groups(harness, children, own_process_group()) {
        signal_group(group, signal);
    }
}

/// The groups [`signal_tree`] signals: the harness's, then each distinct
/// child group, minus this process's own group and the reserved 0 and 1.
#[cfg(unix)]
pub(crate) fn target_groups(
    harness: libc::pid_t,
    children: Vec<libc::pid_t>,
    own_group: libc::pid_t,
) -> Vec<libc::pid_t> {
    let mut groups = Vec::new();
    for group in std::iter::once(harness).chain(children) {
        if group > 1 && group != own_group && !groups.contains(&group) {
            groups.push(group);
        }
    }
    groups
}

#[cfg(unix)]
#[allow(unsafe_code)] // libc::kill; see the SAFETY comment
fn signal_group(group: libc::pid_t, signal: libc::c_int) {
    // SAFETY: plain signal dispatch to a process group this process
    // created (or one a child of its own child created); no memory is
    // shared with the callee. A negative pid addresses the group.
    unsafe {
        libc::kill(-group, signal);
    }
}

#[cfg(unix)]
#[allow(unsafe_code)] // libc::getpgrp; see the SAFETY comment
fn own_process_group() -> libc::pid_t {
    // SAFETY: getpgrp takes no arguments and cannot fail.
    unsafe { libc::getpgrp() }
}

/// The process groups of `parent`'s direct children, excluding `parent`'s
/// own group.
///
/// Linux reads `/proc`; macOS asks libproc. Elsewhere the list is empty and
/// only the harness's own group is signalled.
///
/// On Linux this is a synchronous scan of every `/proc/<pid>/stat`, run from
/// the forced paths (including `Drop`, often on a tokio worker). That is
/// quick on an ordinary host, but on one with many thousands of processes (a
/// busy container host or CI runner) it can stall the dropping thread
/// noticeably.
#[cfg(target_os = "linux")]
pub(crate) fn child_process_groups(parent: libc::pid_t) -> Vec<libc::pid_t> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut groups = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|s| s.parse::<libc::pid_t>().ok()) else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            continue;
        };
        if let Some((ppid, pgrp)) = parse_proc_stat(&stat)
            && ppid == parent
            && pgrp != parent
            && !groups.contains(&pgrp)
        {
            groups.push(pgrp);
        }
    }
    groups
}

/// Parses `(ppid, pgrp)` from a `/proc/<pid>/stat` line. The command name
/// in parentheses may itself contain spaces and parentheses, so the fields
/// are read after the *last* `)`: state, ppid, pgrp, ...
#[cfg(any(target_os = "linux", test))]
pub(crate) fn parse_proc_stat(stat: &str) -> Option<(i32, i32)> {
    let rest = &stat[stat.rfind(')')? + 1..];
    let mut fields = rest.split_whitespace();
    let _state = fields.next()?;
    let ppid = fields.next()?.parse().ok()?;
    let pgrp = fields.next()?.parse().ok()?;
    Some((ppid, pgrp))
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // libproc and getpgid; see the SAFETY comments
pub(crate) fn child_process_groups(parent: libc::pid_t) -> Vec<libc::pid_t> {
    const CAPACITY: usize = 4096;
    let mut pids = vec![0 as libc::pid_t; CAPACITY];
    let bytes = libc::c_int::try_from(CAPACITY * std::mem::size_of::<libc::pid_t>())
        .expect("buffer size fits in c_int");
    // SAFETY: the buffer is valid for `bytes` bytes and outlives the call;
    // libproc writes at most that many.
    let returned = unsafe { libc::proc_listchildpids(parent, pids.as_mut_ptr().cast(), bytes) };
    let Ok(returned) = usize::try_from(returned) else {
        return Vec::new();
    };
    // Documented as a count of pids, but some releases have returned a byte
    // count. The buffer was zeroed, so reading up to the smaller bound and
    // skipping zeros is right either way.
    let mut groups = Vec::new();
    for &pid in pids.iter().take(returned.min(CAPACITY)) {
        if pid <= 0 {
            continue;
        }
        // SAFETY: getpgid only reads the target's process table entry.
        let group = unsafe { libc::getpgid(pid) };
        if group > 0 && group != parent && !groups.contains(&group) {
            groups.push(group);
        }
    }
    groups
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
pub(crate) fn child_process_groups(_parent: libc::pid_t) -> Vec<libc::pid_t> {
    Vec::new()
}

#[cfg(test)]
#[path = "process_tree_tests.rs"]
mod tests;
