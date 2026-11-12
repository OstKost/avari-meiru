/// `starttime` is field 22 of `/proc/pid/stat` (clock ticks), after the comm
/// field which may contain spaces. The value is an identity token, not wall time.
pub fn parse_stat_start_ticks(stat: &str) -> Option<u64> {
    let rest = stat.rsplit_once(')')?.1.trim_start();
    let mut fields = rest.split_whitespace();
    let ticks = fields.nth(19)?;
    let ticks: u64 = ticks.parse().ok()?;
    if ticks == 0 {
        None
    } else {
        Some(ticks)
    }
}

/// State is the first field after `comm`. `Z` is a zombie and `X` is dead.
/// A parent that has not called wait still sees the same starttime.
pub fn parse_stat_state(stat: &str) -> Option<char> {
    let rest = stat.rsplit_once(')')?.1.trim_start();
    rest.chars().next().filter(|state| state.is_ascii_alphabetic())
}

pub fn stat_process_dead(stat: &str) -> bool {
    matches!(parse_stat_state(stat), Some('Z' | 'X'))
}

#[cfg(target_os = "linux")]
pub fn process_start_ticks(pid: u32) -> Option<u64> {
    if pid == 0 {
        return None;
    }
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    parse_stat_start_ticks(&text)
}

#[cfg(target_os = "linux")]
pub fn process_uid(pid: u32) -> Option<u32> {
    status_id(pid, "Uid:")
}

#[cfg(target_os = "linux")]
pub fn process_gid(pid: u32) -> Option<u32> {
    status_id(pid, "Gid:")
}

#[cfg(target_os = "linux")]
fn status_id(pid: u32, prefix: &str) -> Option<u32> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(prefix) {
            let first = rest.split_whitespace().next()?;
            return first.parse().ok();
        }
    }
    None
}

/// `/proc/pid/cmdline` arguments. Empty and interior NUL bytes are not tokens.
pub fn cmdline_tokens(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect()
}

#[cfg(target_os = "linux")]
pub fn process_argv(pid: u32) -> Option<Vec<String>> {
    if pid == 0 {
        return None;
    }
    let bytes = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let args = cmdline_tokens(&bytes);
    if args.is_empty() {
        None
    } else {
        Some(args)
    }
}

#[cfg(target_os = "linux")]
pub fn process_exe(pid: u32) -> Option<std::path::PathBuf> {
    let path = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    if path.as_os_str().is_empty() {
        None
    } else {
        Some(path)
    }
}

#[cfg(target_os = "linux")]
pub fn process_alive(pid: u32, start_ticks: u64) -> bool {
    if pid == 0 || start_ticks == 0 {
        return false;
    }
    let Ok(text) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    if stat_process_dead(&text) {
        return false;
    }
    parse_stat_start_ticks(&text) == Some(start_ticks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_start_ticks_after_spaced_comm() {
        let stat =
            "42 (mieru helper) S 1 42 42 0 -1 4194560 1 2 3 4 5 6 7 8 20 0 1 0 987654 100 50";
        assert_eq!(parse_stat_start_ticks(stat), Some(987654));
    }

    #[test]
    fn rejects_truncated_stat() {
        assert!(parse_stat_start_ticks("1 (init) S 0 1").is_none());
        assert!(parse_stat_start_ticks("no paren").is_none());
        assert!(parse_stat_state("no paren").is_none());
    }

    #[test]
    fn zombie_stat_is_not_a_running_process() {
        let zombie =
            "42 (mieru helper) Z 1 42 42 0 -1 4194560 1 2 3 4 5 6 7 8 20 0 1 0 987654 100 50";
        let alive =
            "42 (mieru helper) S 1 42 42 0 -1 4194560 1 2 3 4 5 6 7 8 20 0 1 0 987654 100 50";
        assert_eq!(parse_stat_state(zombie), Some('Z'));
        assert!(stat_process_dead(zombie));
        assert_eq!(parse_stat_start_ticks(zombie), Some(987654));
        assert!(!stat_process_dead(alive));
        assert!(stat_process_dead(
            "7 (gone) X 1 7 7 0 -1 0 0 0 0 0 0 0 0 20 0 1 0 50 0 0"
        ));
    }

    #[test]
    fn cmdline_tokens_keep_exact_arguments() {
        let bytes = b"/usr/lib/mieru/mieru-helper\0--session\0abc\0--owner-pid\x004321\0";
        assert_eq!(
            cmdline_tokens(bytes),
            vec![
                "/usr/lib/mieru/mieru-helper",
                "--session",
                "abc",
                "--owner-pid",
                "4321"
            ]
        );
        assert!(cmdline_tokens(b"\0\0").is_empty());
    }
}
