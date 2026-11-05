use super::*;
use std::io::{BufRead, BufReader};
use std::time::{Duration, Instant};

#[test]
fn test_parse_proc_stat_reads_fields_after_the_last_paren() {
    // A command name may contain spaces and parentheses of its own.
    let stat = "4242 (weird ) name (x)) S 4100 4242 4100 0 -1 4194560 100 0 0 0";
    assert_eq!(parse_proc_stat(stat), Some((4100, 4242)));
}

#[test]
fn test_parse_proc_stat_rejects_truncated_lines() {
    assert_eq!(parse_proc_stat("4242 (sleep) S"), None);
    assert_eq!(parse_proc_stat("no parens at all"), None);
}

fn is_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// The shape the harness creates: a child that moves its own child into a
/// separate process group. Signalling the parent's group alone misses that
/// grandchild; `signal_tree` must find and stop it too.
#[test]
fn test_signal_tree_reaches_a_child_in_its_own_process_group() {
    use std::os::unix::process::CommandExt;

    let mut parent = std::process::Command::new("python3")
        .args([
            "-c",
            "import subprocess, sys, time\n\
             p = subprocess.Popen(['sleep', '61.731'], start_new_session=True)\n\
             print(p.pid, flush=True)\n\
             time.sleep(120)",
        ])
        .stdout(std::process::Stdio::piped())
        .process_group(0)
        .spawn()
        .expect("python3 is needed for this test");
    let mut line = String::new();
    BufReader::new(parent.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let grandchild: u32 = line.trim().parse().expect("grandchild pid");
    assert!(is_alive(grandchild));

    let parent_pid = libc::pid_t::try_from(parent.id()).unwrap();
    let groups = child_process_groups(parent_pid);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    assert!(
        groups.contains(&libc::pid_t::try_from(grandchild).unwrap()),
        "the grandchild's own group must be found through its parent, got {groups:?}"
    );

    signal_tree(parent.id(), libc::SIGKILL);
    parent.wait().unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    while is_alive(grandchild) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let survived = is_alive(grandchild);
    if survived {
        // Don't leak it past a failing assertion.
        let _ = std::process::Command::new("kill")
            .args(["-9", &grandchild.to_string()])
            .status();
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    assert!(!survived, "a process in its own group outlived signal_tree");
}

#[test]
fn test_target_groups_dedups_and_skips_reserved_and_own_groups() {
    // Harness first, each child group once, never 0, 1 or our own group.
    assert_eq!(
        target_groups(500, vec![600, 500, 600, 1, 0, 77, 700], 77),
        vec![500, 600, 700]
    );
    // A harness that somehow shares our group is not signalled through it.
    assert_eq!(target_groups(77, vec![600], 77), vec![600]);
}
