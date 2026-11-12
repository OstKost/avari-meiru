use std::path::{Path, PathBuf};

use std::os::unix::fs::MetadataExt;

/// Fixed install locations. Packaging copies bits here; the helper never
/// accepts a replacement path from the GUI.
pub const HELPER_PATH: &str = "/usr/lib/mieru/mieru-helper";
pub const SIDECAR_PATH: &str = "/usr/lib/mieru/sing-box";
pub const POLICY_PATH: &str = "/usr/share/polkit-1/actions/app.mieru.policy";
pub const POLKIT_ACTION: &str = "app.mieru.desktop.helper";
pub const PKEXEC_PATH: &str = "/usr/bin/pkexec";

/// Address sing-box assigns as the next host of `172.19.0.1/30`.
pub const TUN_DNS: &str = "172.19.0.2";

pub fn is_session_id(value: &str) -> bool {
    value.len() == 32 && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

pub fn is_tun_name(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("tun") else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit())
}

pub fn runtime_session_dir(uid: u32, session: &str) -> Result<PathBuf, String> {
    if uid == 0 {
        return Err("the desktop app must not run as root".into());
    }
    if !is_session_id(session) {
        return Err("session id is invalid".into());
    }
    Ok(PathBuf::from(format!("/run/user/{uid}/mieru/{session}")))
}

pub fn session_socket(uid: u32, session: &str) -> Result<PathBuf, String> {
    Ok(runtime_session_dir(uid, session)?.join("helper.sock"))
}

pub fn root_session_dir(session: &str) -> Result<PathBuf, String> {
    if !is_session_id(session) {
        return Err("session id is invalid".into());
    }
    Ok(PathBuf::from(format!("/run/mieru/{session}")))
}

/// argv after the fixed helper binary. No profile fields are accepted.
pub fn helper_args(session: &str, owner_pid: u32) -> Result<Vec<String>, String> {
    if !is_session_id(session) {
        return Err("session id is invalid".into());
    }
    if owner_pid < 2 {
        return Err("refusing to use a system pid".into());
    }
    Ok(vec![
        "--session".into(),
        session.into(),
        "--owner-pid".into(),
        owner_pid.to_string(),
    ])
}

pub fn unix_mode_locked(uid: u32, mode: u32) -> bool {
    uid == 0 && (mode & 0o022) == 0
}

/// Docker Desktop Rosetta and qemu-user put the interpreter in `/proc/pid/exe`.
/// The file name is the check. A path that merely contains these letters is not.
pub fn exe_file_is_translator(exe: &Path) -> bool {
    match exe.file_name().and_then(|name| name.to_str()) {
        Some("rosetta" | "rosetta-wrapper") => true,
        Some(name) if name.starts_with("qemu-") => true,
        _ => false,
    }
}

/// Whole argv element. A longer argument that only contains `token` does not count.
pub fn argv_has_exact_token(argv: &[String], token: &str) -> bool {
    !token.is_empty() && argv.iter().any(|arg| arg == token)
}

/// Native exe equals `installed`. A translator is accepted only when `installed`
/// is one whole argv element. This does not accept an arbitrary interpreter pid.
pub fn installed_exe_argv_matches(exe: &Path, argv: &[String], installed: &str) -> bool {
    if installed.is_empty() {
        return false;
    }
    if exe == Path::new(installed) {
        return true;
    }
    exe_file_is_translator(exe) && argv_has_exact_token(argv, installed)
}

fn flag_value<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    argv.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

fn session_owner_flags(argv: &[String], session: &str, owner_pid: u32) -> bool {
    if owner_pid < 2 || !is_session_id(session) {
        return false;
    }
    let owner = owner_pid.to_string();
    flag_value(argv, "--session") == Some(session)
        && flag_value(argv, "--owner-pid") == Some(owner.as_str())
}

/// `argv[0]` is the installed helper. `--session` and `--owner-pid` are the
/// next arguments of those exact flags, not substrings and not bare tokens.
pub fn native_helper_argv(argv: &[String], session: &str, owner_pid: u32) -> bool {
    argv.first().map(String::as_str) == Some(HELPER_PATH)
        && session_owner_flags(argv, session, owner_pid)
}

/// `argv[0]` is a translator path. A later whole argument is `HELPER_PATH`,
/// and the same flag pairs follow. A path that only contains those letters
/// does not count.
pub fn interpreter_helper_argv(argv: &[String], session: &str, owner_pid: u32) -> bool {
    let Some(first) = argv.first() else {
        return false;
    };
    if !exe_file_is_translator(Path::new(first.as_str())) {
        return false;
    }
    argv.iter().skip(1).any(|arg| arg == HELPER_PATH)
        && session_owner_flags(argv, session, owner_pid)
}

/// Native helper: exe is `HELPER_PATH`. A readable translator still needs the
/// native or interpreter argv form above.
pub fn helper_process_matches(exe: &Path, argv: &[String], session: &str, owner_pid: u32) -> bool {
    if exe == Path::new(HELPER_PATH) {
        return true;
    }
    if !exe_file_is_translator(exe) {
        return false;
    }
    native_helper_argv(argv, session, owner_pid) || interpreter_helper_argv(argv, session, owner_pid)
}

/// `/proc/pid/exe` of a root helper is often unreadable (EACCES) on native
/// Linux and under Rosetta. Native argv0 is `HELPER_PATH`. A translator is
/// argv0. Both need the flag pairs. `installed_ok` is the root-owned executable
/// check of that fixed path.
pub fn helper_unreadable_exe_matches(
    argv: &[String],
    session: &str,
    owner_pid: u32,
    installed_ok: bool,
) -> bool {
    if !installed_ok {
        return false;
    }
    native_helper_argv(argv, session, owner_pid) || interpreter_helper_argv(argv, session, owner_pid)
}

/// `SO_PEERCRED` on the connected socket. The Hello JSON pid is not enough.
pub fn socket_peer_accepts(peer_uid: u32, peer_pid: u32, hello_pid: u32) -> bool {
    peer_uid == 0 && hello_pid >= 2 && peer_pid == hello_pid
}

/// Root-owned regular file whose parents are not writable by group or other.
/// Symlinks are rejected so a user-writable directory cannot redirect exec.
pub fn assert_root_owned_tree(path: &Path) -> Result<(), String> {
    let meta =
        std::fs::symlink_metadata(path).map_err(|_| "installed file is missing".to_string())?;
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        return Err("installed file is missing".into());
    }
    if !unix_mode_locked(meta.uid(), meta.mode()) {
        return Err("installed file is not root-owned".into());
    }
    if meta.mode() & 0o111 == 0 {
        return Err("installed file is not executable".into());
    }
    let mut current = path.to_path_buf();
    while let Some(parent) = current.parent() {
        if parent.as_os_str().is_empty() || parent == Path::new("/") {
            break;
        }
        let parent_meta = std::fs::symlink_metadata(parent)
            .map_err(|_| "installed file is missing".to_string())?;
        if parent_meta.file_type().is_symlink() || !parent_meta.is_dir() {
            return Err("installed path has a writable parent".into());
        }
        if !unix_mode_locked(parent_meta.uid(), parent_meta.mode()) {
            return Err("installed path has a writable parent".into());
        }
        current = parent.to_path_buf();
    }
    Ok(())
}

pub fn classify_pkexec_failure(code: Option<i32>, stderr: &str) -> String {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("no authentication agent")
        || lower.contains("authentication agent")
        || lower.contains("cannot open display")
    {
        return "no polkit authentication agent is available".into();
    }
    if lower.contains("no such file")
        || lower.contains("not found")
        || code == Some(127) && lower.contains("mieru-helper")
    {
        return "the installed VPN helper is missing".into();
    }
    if code == Some(126)
        || lower.contains("dismissed")
        || lower.contains("not authorized")
        || lower.contains("authorization canceled")
        || lower.contains("authorization cancelled")
        || lower.contains("request dismissed")
    {
        return "administrator privileges were not granted".into();
    }
    if code == Some(127) {
        return "could not start the elevated core".into();
    }
    "could not start the elevated core".into()
}

pub fn tun_name_from_log(log: &str) -> Option<String> {
    for line in log.lines() {
        let Some(idx) = line.rfind("started at ") else {
            continue;
        };
        let rest = line[idx + "started at ".len()..].trim();
        let name = rest.split_whitespace().next().unwrap_or(rest);
        let name = name.trim_matches(|ch: char| ch == ',' || ch == '.' || ch == ')');
        if is_tun_name(name) {
            return Some(name.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_args_carry_no_profile_secrets() {
        let args = helper_args("0123456789abcdef0123456789abcdef", 4321).unwrap();
        let joined = args.join(" ");
        assert!(!joined.contains("example-password"));
        assert!(!joined.contains("GgQIARAK"));
        assert!(!joined.contains("sing-box"));
        assert!(!joined.contains("-c"));
        assert_eq!(
            args,
            vec![
                "--session",
                "0123456789abcdef0123456789abcdef",
                "--owner-pid",
                "4321"
            ]
        );
        assert!(helper_args("../etc/passwd", 10).is_err());
        assert!(helper_args("0123456789abcdef0123456789abcdef", 1).is_err());
    }

    #[test]
    fn socket_path_is_derived() {
        let path = session_socket(501, "0123456789abcdef0123456789abcdef").unwrap();
        assert_eq!(
            path,
            PathBuf::from("/run/user/501/mieru/0123456789abcdef0123456789abcdef/helper.sock")
        );
        assert!(session_socket(0, "0123456789abcdef0123456789abcdef").is_err());
    }

    #[test]
    fn helper_translator_requires_exact_path_session_and_owner_pid() {
        let session = "0123456789abcdef0123456789abcdef";
        let owner = 4321u32;
        let argv = vec![
            "/run/rosetta/rosetta".into(),
            HELPER_PATH.into(),
            "--session".into(),
            session.into(),
            "--owner-pid".into(),
            owner.to_string(),
        ];
        assert!(helper_process_matches(
            Path::new(HELPER_PATH),
            &[],
            session,
            owner
        ));
        assert!(helper_process_matches(
            Path::new("/run/rosetta/rosetta"),
            &argv,
            session,
            owner
        ));
        assert!(helper_process_matches(
            Path::new("/usr/bin/rosetta-wrapper"),
            &argv,
            session,
            owner
        ));
        assert!(helper_process_matches(
            Path::new("/usr/bin/qemu-x86_64-static"),
            &argv,
            session,
            owner
        ));
        let embedded = vec![format!("{HELPER_PATH}-evil"), session.into(), owner.to_string()];
        assert!(!helper_process_matches(
            Path::new("/run/rosetta/rosetta"),
            &embedded,
            session,
            owner
        ));
        let prefixed = vec![
            format!("/tmp{HELPER_PATH}"),
            session.into(),
            owner.to_string(),
        ];
        assert!(!helper_process_matches(
            Path::new("/run/rosetta/rosetta"),
            &prefixed,
            session,
            owner
        ));
        assert!(!helper_process_matches(
            Path::new("/run/rosetta/rosetta"),
            &argv,
            &format!("{session}aa"),
            owner
        ));
        assert!(!helper_process_matches(
            Path::new("/run/rosetta/rosetta"),
            &argv,
            session,
            43210
        ));
        let seen_by_user = vec![
            "/run/rosetta/rosetta".into(),
            HELPER_PATH.into(),
            HELPER_PATH.into(),
            "--session".into(),
            session.into(),
            "--owner-pid".into(),
            owner.to_string(),
        ];
        assert!(helper_unreadable_exe_matches(
            &seen_by_user, session, owner, true
        ));
        let native_only = vec![
            HELPER_PATH.into(),
            "--session".into(),
            session.into(),
            "--owner-pid".into(),
            owner.to_string(),
        ];
        assert!(helper_unreadable_exe_matches(
            &native_only, session, owner, true
        ));
        assert!(helper_process_matches(
            Path::new("/run/rosetta/rosetta"),
            &native_only,
            session,
            owner
        ));
        assert!(!helper_unreadable_exe_matches(
            &native_only, session, owner, false
        ));
        assert!(!helper_unreadable_exe_matches(
            &[
                "/usr/bin/qemu-x86_64".into(),
                HELPER_PATH.into(),
                session.into(),
                owner.to_string(),
            ],
            session,
            owner,
            true
        ));
        assert!(!helper_unreadable_exe_matches(
            &[
                "/run/rosetta/rosetta".into(),
                format!("{HELPER_PATH}-evil"),
                session.into(),
                owner.to_string(),
            ],
            session,
            owner,
            true
        ));
        assert!(!helper_unreadable_exe_matches(
            &[
                "/opt/rosetta-extra".into(),
                HELPER_PATH.into(),
                session.into(),
                owner.to_string(),
            ],
            session,
            owner,
            true
        ));
        assert!(!helper_unreadable_exe_matches(
            &seen_by_user, session, 43210, true
        ));
        assert!(!helper_process_matches(
            Path::new("/bin/sleep"),
            &argv,
            session,
            owner
        ));
        assert!(!helper_process_matches(
            Path::new("/usr/bin/qemu-x86_64"),
            &argv,
            "not-a-session",
            owner
        ));
        assert!(!installed_exe_argv_matches(
            Path::new("/run/rosetta/rosetta"),
            &[format!("{SIDECAR_PATH}.bak")],
            SIDECAR_PATH
        ));
        assert!(installed_exe_argv_matches(
            Path::new(SIDECAR_PATH),
            &[],
            SIDECAR_PATH
        ));
    }

    #[test]
    fn native_unreadable_argv_and_peer_reject_loose_forms() {
        let session = "0123456789abcdef0123456789abcdef";
        let owner = 4321u32;
        let native = vec![
            HELPER_PATH.into(),
            "--session".into(),
            session.into(),
            "--owner-pid".into(),
            owner.to_string(),
        ];
        assert!(native_helper_argv(&native, session, owner));
        assert!(!native_helper_argv(
            &[
                format!("{HELPER_PATH}-evil"),
                "--session".into(),
                session.into(),
                "--owner-pid".into(),
                owner.to_string(),
            ],
            session,
            owner
        ));
        assert!(!native_helper_argv(
            &[
                HELPER_PATH.into(),
                format!("--session={session}"),
                "--owner-pid".into(),
                owner.to_string(),
            ],
            session,
            owner
        ));
        assert!(!native_helper_argv(
            &[
                HELPER_PATH.into(),
                "--session".into(),
                "nope".into(),
                session.into(),
                "--owner-pid".into(),
                owner.to_string(),
            ],
            session,
            owner
        ));
        assert!(!native_helper_argv(
            &[
                HELPER_PATH.into(),
                "--session".into(),
                session.into(),
                format!("--owner-pid={owner}"),
            ],
            session,
            owner
        ));
        assert!(!native_helper_argv(&native, session, 43210));
        assert!(!helper_unreadable_exe_matches(&native, session, owner, false));
        assert!(!helper_unreadable_exe_matches(
            &[
                "/bin/sleep".into(),
                HELPER_PATH.into(),
                "--session".into(),
                session.into(),
                "--owner-pid".into(),
                owner.to_string(),
            ],
            session,
            owner,
            true
        ));
        let interpreter = vec![
            "/usr/bin/qemu-x86_64".into(),
            HELPER_PATH.into(),
            "--session".into(),
            session.into(),
            "--owner-pid".into(),
            owner.to_string(),
        ];
        assert!(interpreter_helper_argv(&interpreter, session, owner));
        assert!(helper_unreadable_exe_matches(
            &interpreter, session, owner, true
        ));
        assert!(!interpreter_helper_argv(
            &[
                "/opt/my-rosetta".into(),
                HELPER_PATH.into(),
                "--session".into(),
                session.into(),
                "--owner-pid".into(),
                owner.to_string(),
            ],
            session,
            owner
        ));
        assert!(!interpreter_helper_argv(
            &[
                "/usr/bin/qemu-x86_64".into(),
                format!("{HELPER_PATH}-evil"),
                "--session".into(),
                session.into(),
                "--owner-pid".into(),
                owner.to_string(),
            ],
            session,
            owner
        ));
        assert!(socket_peer_accepts(0, 48, 48));
        assert!(!socket_peer_accepts(1001, 48, 48));
        assert!(!socket_peer_accepts(0, 46, 48));
        assert!(!socket_peer_accepts(0, 1, 1));
        assert!(!socket_peer_accepts(0, 48, 49));
    }

    #[test]
    fn classifies_polkit_denial_and_missing_agent() {
        assert_eq!(
            classify_pkexec_failure(Some(126), "Error: request dismissed"),
            "administrator privileges were not granted"
        );
        assert_eq!(
            classify_pkexec_failure(
                Some(127),
                "Error executing command as another user: No authentication agent found."
            ),
            "no polkit authentication agent is available"
        );
        assert_eq!(
            classify_pkexec_failure(Some(127), "pkexec: mieru-helper: No such file or directory"),
            "the installed VPN helper is missing"
        );
        assert_eq!(
            classify_pkexec_failure(Some(1), "something else"),
            "could not start the elevated core"
        );
    }

    #[test]
    fn mode_lock_rejects_group_write() {
        assert!(unix_mode_locked(0, 0o755));
        assert!(!unix_mode_locked(0, 0o775));
        assert!(!unix_mode_locked(501, 0o755));
    }

    #[test]
    fn tun_name_from_ready_line() {
        let log = "INFO inbound/tun[tun-in]: started at tun0\n";
        assert_eq!(tun_name_from_log(log).as_deref(), Some("tun0"));
        assert!(tun_name_from_log("started at eth0").is_none());
        assert!(!is_tun_name("tun"));
        assert!(!is_tun_name("tun0;rm"));
    }

    #[test]
    fn real_system_binary_is_root_locked_when_present() {
        let candidate = if Path::new("/usr/bin/true").is_file() {
            Path::new("/usr/bin/true")
        } else if Path::new("/bin/true").is_file() {
            Path::new("/bin/true")
        } else {
            return;
        };
        // /bin may be a symlink. Only assert when the walk's preconditions hold.
        if assert_root_owned_tree(candidate).is_err() {
            let meta = std::fs::symlink_metadata(candidate).unwrap();
            if !unix_mode_locked(meta.uid(), meta.mode()) {
                panic!("system binary is not root-locked");
            }
        }
    }
}
