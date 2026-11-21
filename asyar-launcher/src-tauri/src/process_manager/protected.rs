//! "Is this an OS-critical process?" classifier.

use crate::process_manager::types::RawProcess;

/// Linux rules: init, kernel threads, and root-owned sbin daemons.
pub fn classify(p: &RawProcess) -> bool {
    if p.pid == 1 {
        return true; // init / systemd
    }
    if p.parent_pid == Some(2) || p.pid == 2 {
        return true; // kernel threads (kthreadd is pid 2)
    }
    let root = p.owner == "root" || p.owner.is_empty();
    root && (p.exe_path.starts_with("/usr/sbin/") || p.exe_path.starts_with("/sbin/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(name: &str, owner: &str, exe: &str, ppid: Option<u32>) -> RawProcess {
        RawProcess {
            pid: 100,
            parent_pid: ppid,
            name: name.into(),
            cpu_percent: 0.0,
            memory_bytes: 0,
            exe_path: exe.into(),
            owner: owner.into(),
        }
    }

    #[test]
    fn linux_init_and_kernel_threads_are_protected() {
        let mut init = raw("systemd", "root", "/usr/lib/systemd/systemd", None);
        init.pid = 1;
        assert!(classify(&init));
        assert!(classify(&raw("kworker/0:1", "root", "", Some(2)))); // ppid 2 = kthreadd
    }

    #[test]
    fn linux_user_app_is_not_protected() {
        let mut p = raw("code", "alice", "/usr/share/code/code", Some(1234));
        p.pid = 1234;
        assert!(!classify(&p));
    }
}
