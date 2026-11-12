/// Quote one shell word in single quotes. Control characters are rejected
/// because they can break the AppleScript string that wraps the command.
pub fn shell_single_quote(value: &str) -> Result<String, String> {
    if value.chars().any(|ch| ch.is_control()) {
        return Err("a path contains a control character".into());
    }
    let mut out = String::from("'");
    for ch in value.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    Ok(out)
}

pub fn applescript_escape(value: &str) -> Result<String, String> {
    if value.chars().any(|ch| ch.is_control()) {
        return Err("the elevated command contains a control character".into());
    }
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch == '\\' || ch == '"' {
            out.push('\\');
        }
        out.push(ch);
    }
    Ok(out)
}

pub fn format_pid(pid: u32) -> Result<String, String> {
    if pid < 2 {
        return Err("refusing to use a system pid".into());
    }
    let text = pid.to_string();
    if !text.chars().all(|ch| ch.is_ascii_digit()) {
        return Err("pid is not numeric".into());
    }
    Ok(text)
}

/// Shell command that starts the supervisor and prints its pid.
/// Profile fields are not accepted here.
pub fn supervisor_launch_shell(
    binary: &str,
    runtime: &str,
    owner_pid: u32,
) -> Result<String, String> {
    let pid = format_pid(owner_pid)?;
    let log_path = format!("{runtime}/supervisor.log");
    let inner = format!(
        "set -eu; ( trap '' HUP; exec {} --mieru-supervisor --runtime {} --owner-pid {} </dev/null >>{} 2>&1 ) & echo $!",
        shell_single_quote(binary)?,
        shell_single_quote(runtime)?,
        pid,
        shell_single_quote(&log_path)?,
    );
    Ok(format!("/bin/bash -c {}", shell_single_quote(&inner)?))
}

pub fn kill_shell(pid: u32, signal: &str) -> Result<String, String> {
    if signal != "TERM" && signal != "KILL" {
        return Err("refusing to send an unexpected signal".into());
    }
    let pid = format_pid(pid)?;
    Ok(format!("/bin/kill -{signal} {pid}"))
}

pub fn admin_applescript(shell: &str) -> Result<String, String> {
    Ok(format!(
        "do shell script \"{}\" with administrator privileges",
        applescript_escape(shell)?
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_spaces_and_single_quotes() {
        assert_eq!(shell_single_quote("/tmp/a b").unwrap(), "'/tmp/a b'");
        assert_eq!(shell_single_quote("a'b;rm").unwrap(), "'a'\\''b;rm'");
    }

    #[test]
    fn rejects_newlines() {
        assert!(shell_single_quote("/tmp/a\nb").is_err());
        assert!(applescript_escape("line\nbreak").is_err());
    }

    #[test]
    fn launch_shell_does_not_include_profile_secrets() {
        let shell = supervisor_launch_shell(
            "/Applications/Mieru.app/Contents/MacOS/mieru-desktop",
            "/Users/example/Library/Application Support/Mieru/sessions/0123456789abcdef0123456789abcdef",
            4321,
        )
        .unwrap();
        let secret = "example-password";
        let pattern = "GgQIARAK";
        assert!(!shell.contains(secret));
        assert!(!shell.contains(pattern));
        assert!(shell.contains("--mieru-supervisor"));
        assert!(shell.contains("--owner-pid 4321"));
        assert!(shell.contains("trap"));
        assert!(shell.contains("HUP"));
        assert!(shell.contains("</dev/null"));
        assert!(!shell.contains("nohup"));
        assert!(!shell.contains("sing-box run"));
        let script = admin_applescript(&shell).unwrap();
        assert!(script.starts_with("do shell script \""));
        assert!(script.contains("with administrator privileges"));
        assert!(!script.contains('\n'));
    }

    #[test]
    fn kill_shell_is_numeric() {
        assert_eq!(kill_shell(99, "TERM").unwrap(), "/bin/kill -TERM 99");
        assert!(kill_shell(1, "TERM").is_err());
        assert!(kill_shell(99, "HUP").is_err());
    }
}
