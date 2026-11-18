use super::*;

#[test]
fn process_inventory_refuses_the_app_and_init_process_ids() {
    for pid in [0, 1, std::process::id()] {
        let error = crate::test_block_on(stop_process(pid, ProcessStartTime(1)))
            .expect_err("protected process IDs are refused");
        assert_eq!(error.code, ChatErrorCode::Validation, "{pid}");
    }
}

#[cfg(target_os = "linux")]
mod linux_tests {
    use super::super::linux::{ProcessStat, descendants, parse_stat};
    use super::*;
    use std::os::unix::process::CommandExt;

    fn spawn_sleeper() -> std::process::Child {
        let mut command = std::process::Command::new("/bin/sleep");
        command.arg("30").process_group(0);
        command.spawn().expect("sleep starts")
    }

    fn listed(pid: u32) -> bool {
        list_processes()
            .roots
            .iter()
            .any(|record| record.pid == pid)
    }

    #[test]
    fn process_inventory_parses_names_with_spaces_and_parentheses() {
        let mut line = "4242 (a b) (c)) S 7 4242 4242 0 -1 0".to_string();
        for field in 10..=21 {
            line.push_str(&format!(" {field}"));
        }
        line.push_str(" 987654 rest");
        let stat = parse_stat(&line).expect("stat parses");
        assert_eq!(stat.name, "a b) (c)");
        assert_eq!(stat.parent_pid, 7);
        assert_eq!(stat.process_group, 4242);
        assert_eq!(stat.start_time, ProcessStartTime(987654));
        assert_eq!(stat.own_ticks, 14 + 15);
        assert_eq!(stat.exited_children_ticks, 16 + 17);
        assert!(!stat.zombie);
        assert!(parse_stat("4242 (cut").is_none());
    }

    #[test]
    fn process_inventory_registers_lists_stops_and_forgets_on_drop() {
        let mut child = spawn_sleeper();
        let pid = child.id();
        let registration = register_process(
            pid,
            Path::new("/bin/sleep"),
            Some("Codex"),
            ChatProcessPurpose::Session,
            Some(ResourceScope::Scoped),
        );
        let record = list_processes()
            .roots
            .into_iter()
            .find(|record| record.pid == pid)
            .expect("registered process is listed");
        assert_eq!(record.executable_name, "sleep");
        assert_eq!(record.provider_instance_label.as_deref(), Some("Codex"));
        assert!(record.stoppable);
        assert_eq!(record.resource_scope, Some(ResourceScope::Scoped));
        let start_time = record.start_time.expect("Linux reports a start time");
        crate::test_block_on(stop_process(pid, start_time)).expect("registered process stops");
        child.wait().expect("sleep is reaped");
        assert!(!listed(pid), "a stopped process is not listed");
        drop(registration);
        assert!(inventory().values().all(|record| record.pid != pid));
    }

    #[test]
    fn descendant_cpu_time_counts_busy_children_and_not_the_idle_parent() {
        let mut parent = std::process::Command::new("/bin/sh");
        parent
            .args(["-c", "/bin/sh -c 'while :; do :; done' & wait"])
            .process_group(0);
        let mut child = parent.spawn().expect("shell starts");
        let pid = child.id();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut used = Duration::ZERO;
        while used < Duration::from_millis(100) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
            used = descendant_cpu_time(pid).expect("the parent is running");
        }
        let group = libc::pid_t::try_from(pid).expect("process ID fits");
        crate::process::signal_process_group(group, libc::SIGKILL).expect("group stops");
        child.wait().expect("shell is reaped");
        assert!(
            used >= Duration::from_millis(100),
            "busy child time counts: {used:?}"
        );
        assert!(
            descendant_cpu_time(pid).is_none(),
            "a gone process has no time"
        );
    }

    #[test]
    fn process_inventory_refuses_unregistered_and_mismatched_identities() {
        let mut child = spawn_sleeper();
        let pid = child.id();
        let fresh = current_start_time(pid).expect("start time reads");
        let unregistered = crate::test_block_on(stop_process(pid, fresh))
            .expect_err("an unregistered process is refused");
        assert_eq!(unregistered.code, ChatErrorCode::ProcessNotFound);

        let registration = register_process(
            pid,
            Path::new("/bin/sleep"),
            None,
            ChatProcessPurpose::Terminal,
            None,
        );
        let other = ProcessStartTime(fresh.0 + 1);
        let mismatch = crate::test_block_on(stop_process(pid, other))
            .expect_err("a different start time is refused");
        assert_eq!(mismatch.code, ChatErrorCode::ProcessNotFound);

        let stale = {
            let mut records = inventory();
            let id = NEXT_REGISTRATION.fetch_add(1, Ordering::Relaxed);
            let mut record = records
                .values()
                .find(|record| record.pid == pid)
                .cloned()
                .expect("record exists");
            record.start_time = Some(other);
            records.insert(id, record);
            ProcessRegistration { id }
        };
        let changed = crate::test_block_on(stop_process(pid, other))
            .expect_err("a reused process ID is refused");
        assert_eq!(changed.code, ChatErrorCode::ProcessIdentityChanged);
        assert!(alive_as(pid, fresh), "a refused stop sends no signal");
        drop(stale);
        drop(registration);
        child.kill().expect("sleep stops");
        child.wait().expect("sleep is reaped");
    }

    #[test]
    fn process_inventory_bounds_descendants_and_counts_the_rest() {
        let stat = |parent_pid: u32| ProcessStat {
            name: format!("child-of-{parent_pid}"),
            zombie: false,
            parent_pid,
            process_group: parent_pid,
            start_time: ProcessStartTime(1),
            own_ticks: 0,
            exited_children_ticks: 0,
        };
        let processes = BTreeMap::from([
            (10, stat(1)),
            (11, stat(10)),
            (12, stat(10)),
            (13, stat(11)),
            (20, stat(1)),
            (21, stat(20)),
            (30, stat(1)),
        ]);
        let (listed, omitted) = descendants(&[10, 20], &processes, 3);
        assert_eq!(
            listed
                .iter()
                .map(|row| (row.pid, row.parent_pid, row.root_pid))
                .collect::<Vec<_>>(),
            vec![(11, 10, 10), (12, 10, 10), (13, 11, 10)]
        );
        assert_eq!(omitted, 1);
        let (all, none) = descendants(&[10, 20], &processes, MAX_LISTED_PROCESSES);
        assert_eq!(all.len(), 4);
        assert_eq!(none, 0);
    }
}
