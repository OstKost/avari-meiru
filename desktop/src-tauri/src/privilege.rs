use std::process::{Command, Stdio};

use crate::escape::admin_applescript;

pub fn elevate(shell: &str) -> Result<String, String> {
    let script = admin_applescript(shell)?;
    let output = Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .stdin(Stdio::null())
        .output()
        .map_err(|_| "could not request administrator privileges".to_string())?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("user canceled")
        || lower.contains("(-128)")
        || lower.contains("not authorized")
    {
        Err("administrator privileges were not granted".into())
    } else {
        Err("could not start the elevated core".into())
    }
}
