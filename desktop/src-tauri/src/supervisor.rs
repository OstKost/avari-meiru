use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::fsutil;
use crate::identity::{self, ProcessClaim};

const HEARTBEAT_STALE: Duration = Duration::from_secs(90);
const CHILD_TERM_WAIT: Duration = Duration::from_secs(8);

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LaunchFile {
    sing_box: String,
    config: String,
    workdir: String,
    log_path: String,
    owner_pid: u32,
    owner_start_sec: u64,
    owner_uid: u32,
    owner_gid: u32,
    nonce: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StateFile {
    pub supervisor_pid: u32,
    pub supervisor_start: Option<u64>,
    pub singbox_pid: Option<u32>,
    pub singbox_start: Option<u64>,
    pub sing_box: String,
    pub config: String,
    pub nonce: String,
    pub unexpected: bool,
    pub exit_code: Option<i32>,
}

pub fn requested() -> bool {
    std::env::args().nth(1).as_deref() == Some("--mieru-supervisor")
}

pub fn run() -> i32 {
    match run_inner() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("mieru supervisor: {err}");
            1
        }
    }
}

fn run_inner() -> Result<i32, String> {
    let runtime = parse_args()?;
    // The elevated process must outlive osascript. SIGHUP is already ignored
    // by nohup; a new session keeps it out of the GUI process group.
    unsafe {
        let _ = libc::setsid();
    }
    let stop = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stop))
        .map_err(|_| "could not watch for stop signals")?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&stop))
        .map_err(|_| "could not watch for stop signals")?;

    let launch = read_launch(&runtime)?;
    validate_launch(&runtime, &launch)?;
    if !owner_matches(&launch) {
        return Err("the desktop process is not the expected owner".into());
    }
    // Write initial state immediately so GUI can confirm handshake without waiting for child spawn
    let _ = write_state(&runtime, &launch, None, None, false, None);

    let log_file = OpenOptions::new()
        .append(true)
        .open(&launch.log_path)
        .map_err(|_| "core log is missing".to_string())?;
    let log_err = log_file
        .try_clone()
        .map_err(|_| "core log is missing".to_string())?;
    let mut child = Command::new(&launch.sing_box)
        .args([
            "run",
            "-c",
            &launch.config,
            "-D",
            &launch.workdir,
            "--disable-color",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_err))
        .current_dir(&launch.workdir)
        .spawn()
        .map_err(|_| "could not start the core process".to_string())?;

    let singbox_pid = child.id();
    let started = Instant::now();
    let singbox_start = wait_child_identity(&launch, singbox_pid, Duration::from_secs(2));
    write_state(
        &runtime,
        &launch,
        singbox_start.map(|_| singbox_pid),
        singbox_start,
        false,
        None,
    )?;

    let mut last_beat = read_heartbeat(&runtime).unwrap_or(0);
    let mut last_change = Instant::now();
    let mut unexpected = false;
    let mut exit_code = None;

    loop {
        if stop.load(Ordering::SeqCst) || stop_requested(&runtime) || !owner_matches(&launch) {
            break;
        }
        if let Some(beat) = read_heartbeat(&runtime) {
            if beat != last_beat {
                last_beat = beat;
                last_change = Instant::now();
            }
        }
        if started.elapsed() > HEARTBEAT_STALE && last_change.elapsed() > HEARTBEAT_STALE {
            break;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                unexpected = true;
                exit_code = status.code();
                break;
            }
            Ok(None) => {}
            Err(_) => {
                unexpected = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(200));
    }

    let child_exit = stop_child(&mut child, &launch);
    if unexpected {
        exit_code = exit_code.or(child_exit);
    }
    let _ = fs::remove_file(&launch.config);
    let _ = fs::remove_dir_all(&launch.workdir);
    let _ = write_state(
        &runtime,
        &launch,
        Some(singbox_pid),
        singbox_start,
        unexpected,
        exit_code,
    );
    if unexpected {
        Ok(1)
    } else {
        Ok(0)
    }
}

fn parse_args() -> Result<PathBuf, String> {
    let mut args = std::env::args().skip(1);
    let marker = args.next().ok_or("missing supervisor marker")?;
    if marker != "--mieru-supervisor" {
        return Err("unexpected supervisor invocation".into());
    }
    let runtime_flag = args.next().ok_or("missing runtime")?;
    if runtime_flag != "--runtime" {
        return Err("missing runtime".into());
    }
    let runtime = args.next().ok_or("missing runtime path")?;
    let owner_flag = args.next().ok_or("missing owner")?;
    if owner_flag != "--owner-pid" {
        return Err("missing owner".into());
    }
    let owner = args.next().ok_or("missing owner pid")?;
    if args.next().is_some() {
        return Err("unexpected supervisor arguments".into());
    }
    if !owner.chars().all(|ch| ch.is_ascii_digit()) {
        return Err("owner pid is not numeric".into());
    }
    let runtime = PathBuf::from(runtime);
    if !runtime.is_absolute() {
        return Err("runtime path must be absolute".into());
    }
    let launch = read_launch(&runtime)?;
    if launch.owner_pid.to_string() != owner {
        return Err("owner pid does not match the launch file".into());
    }
    Ok(runtime)
}

fn read_launch(runtime: &Path) -> Result<LaunchFile, String> {
    let bytes =
        fs::read(runtime.join("launch.json")).map_err(|_| "launch file is missing".to_string())?;
    serde_json::from_slice(&bytes).map_err(|_| "launch file is invalid".to_string())
}

fn validate_launch(runtime: &Path, launch: &LaunchFile) -> Result<(), String> {
    if launch.owner_uid == 0 {
        return Err("the desktop app must not run as root".into());
    }
    if !is_nonce(&launch.nonce) {
        return Err("launch nonce is invalid".into());
    }
    let runtime = runtime
        .canonicalize()
        .map_err(|_| "runtime directory is missing".to_string())?;
    if runtime.file_name().and_then(|name| name.to_str()) != Some(launch.nonce.as_str()) {
        return Err("runtime directory does not match the launch nonce".into());
    }
    require_inside(&runtime, Path::new(&launch.config), "config.json")?;
    require_inside(&runtime, Path::new(&launch.workdir), "workdir")?;
    require_inside(&runtime, Path::new(&launch.log_path), "core.log")?;
    let _ = launch.owner_gid;
    let sing_box = PathBuf::from(&launch.sing_box);
    let is_valid_name = sing_box
        .file_name()
        .and_then(|n| n.to_str())
        .map_or(false, crate::core_config::is_allowed_sidecar_name);
    if !sing_box.is_absolute() || !is_valid_name {
        return Err("sidecar path is not the bundled core".into());
    }
    if !sing_box.is_file() {
        return Err("sidecar binary is missing".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = sing_box.metadata() {
            if meta.permissions().mode() & 0o111 == 0 {
                return Err("sidecar binary is not executable".into());
            }
        }
    }
    Ok(())
}

fn require_inside(runtime: &Path, path: &Path, file_name: &str) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("a launch path must be absolute".into());
    }
    if !file_name.is_empty() && path.file_name().and_then(|name| name.to_str()) != Some(file_name) {
        return Err("a launch path has an unexpected name".into());
    }
    if !fsutil::is_inside(runtime, path) {
        return Err("a launch path escapes the runtime directory".into());
    }
    Ok(())
}

fn is_nonce(value: &str) -> bool {
    value.len() == 32 && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn owner_matches(launch: &LaunchFile) -> bool {
    let Some(facts) = identity::inspect(launch.owner_pid) else {
        return false;
    };
    let Ok(own_exe) = std::env::current_exe() else {
        return false;
    };
    identity::same_file(&facts.exe, &own_exe) && facts.start_sec == launch.owner_start_sec
}

fn wait_child_identity(launch: &LaunchFile, pid: u32, timeout: Duration) -> Option<u64> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(facts) = identity::inspect(pid) {
            if identity::same_file(&facts.exe, Path::new(&launch.sing_box))
                && facts.args.contains(&launch.config)
            {
                return Some(facts.start_sec);
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn stop_child(child: &mut Child, launch: &LaunchFile) -> Option<i32> {
    if let Ok(Some(status)) = child.try_wait() {
        return status.code();
    }
    let pid = child.id();
    signal_child(pid, launch, libc::SIGTERM);
    let deadline = Instant::now() + CHILD_TERM_WAIT;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return status.code();
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    signal_child(pid, launch, libc::SIGKILL);
    child.wait().ok().and_then(|status| status.code())
}

fn signal_child(pid: u32, launch: &LaunchFile, signal: i32) {
    let Some(facts) = identity::inspect(pid) else {
        return;
    };
    if !identity::same_file(&facts.exe, Path::new(&launch.sing_box))
        || !facts.args.contains(&launch.config)
    {
        return;
    }
    unsafe {
        libc::kill(pid as i32, signal);
    }
}

fn stop_requested(runtime: &Path) -> bool {
    runtime.join("stop").is_file()
}

fn read_heartbeat(runtime: &Path) -> Option<u64> {
    let mut file = fs::File::open(runtime.join("heartbeat")).ok()?;
    let mut text = String::new();
    file.read_to_string(&mut text).ok()?;
    text.trim().parse().ok()
}

fn write_state(
    runtime: &Path,
    launch: &LaunchFile,
    singbox_pid: Option<u32>,
    singbox_start: Option<u64>,
    unexpected: bool,
    exit_code: Option<i32>,
) -> Result<(), String> {
    let state = StateFile {
        supervisor_pid: std::process::id(),
        supervisor_start: identity::start_sec(std::process::id()),
        singbox_pid,
        singbox_start,
        sing_box: launch.sing_box.clone(),
        config: launch.config.clone(),
        nonce: launch.nonce.clone(),
        unexpected,
        exit_code,
    };
    let bytes = serde_json::to_vec_pretty(&state)
        .map_err(|_| "could not store supervisor state".to_string())?;
    let path = runtime.join("state.json");
    // The GUI pre-creates this file as the user. Overwrite it in place so the
    // inode stays readable by the desktop process.
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&path)
        .map_err(|_| "state file is missing".to_string())?;
    file.write_all(&bytes)
        .map_err(|_| "could not store supervisor state".to_string())?;
    file.sync_all().ok();
    Ok(())
}

pub fn confirm_handshake(runtime: &Path, expected_pid: u32, expected_nonce: &str) -> bool {
    let Ok(bytes) = fs::read(runtime.join("state.json")) else {
        return false;
    };
    let Ok(state): Result<StateFile, _> = serde_json::from_slice(&bytes) else {
        return false;
    };
    state.supervisor_pid == expected_pid && state.nonce == expected_nonce
}

pub fn child_claim(runtime: &Path) -> Option<ProcessClaim> {
    let bytes = fs::read(runtime.join("state.json")).ok()?;
    let state: StateFile = serde_json::from_slice(&bytes).ok()?;
    Some(ProcessClaim {
        pid: state.singbox_pid?,
        exe: PathBuf::from(state.sing_box),
        start_sec: state.singbox_start?,
        marker: state.config,
        dir: Some(runtime.to_path_buf()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn validates_sidecar_names_and_rejects_arbitrary_binary() {
        let dir = std::env::temp_dir().join(format!("mieru-sup-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let nonce = "0123456789abcdef0123456789abcdef";
        let runtime = dir.join(nonce);
        fs::create_dir_all(&runtime).unwrap();
        let config = runtime.join("config.json");
        let workdir = runtime.join("workdir");
        let log_path = runtime.join("core.log");
        fs::write(&config, b"{}").unwrap();
        fs::create_dir_all(&workdir).unwrap();
        fs::write(&log_path, b"").unwrap();

        let real_sidecar = crate::core_config::sidecar_path().expect("real sidecar");

        // 1. Valid with dev suffixed name (real_sidecar)
        let mut launch = LaunchFile {
            owner_uid: 501,
            owner_gid: 20,
            owner_pid: std::process::id(),
            owner_start_sec: 1,
            nonce: nonce.to_string(),
            config: config.to_str().unwrap().to_string(),
            workdir: workdir.to_str().unwrap().to_string(),
            log_path: log_path.to_str().unwrap().to_string(),
            sing_box: real_sidecar.to_str().unwrap().to_string(),
        };
        assert!(validate_launch(&runtime, &launch).is_ok());

        // 2. Valid with bundled "sing-box" name
        let bundled_sing_box = runtime.join("sing-box");
        fs::copy(&real_sidecar, &bundled_sing_box).unwrap();
        launch.sing_box = bundled_sing_box.to_str().unwrap().to_string();
        assert!(validate_launch(&runtime, &launch).is_ok());

        // 3. Rejects arbitrary / other binary name (e.g. "evil-binary" or "bash")
        let foreign_binary = runtime.join("evil-binary");
        fs::copy(&real_sidecar, &foreign_binary).unwrap();
        launch.sing_box = foreign_binary.to_str().unwrap().to_string();
        let err = validate_launch(&runtime, &launch).unwrap_err();
        assert_eq!(err, "sidecar path is not the bundled core");

        // 4. Rejects relative path
        launch.sing_box = "sing-box".to_string();
        assert!(validate_launch(&runtime, &launch).is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn is_allowed_sidecar_name_checks() {
        assert!(crate::core_config::is_allowed_sidecar_name("sing-box"));
        let arch_name = crate::core_config::sidecar_file_name().unwrap();
        assert!(crate::core_config::is_allowed_sidecar_name(arch_name));
        assert!(!crate::core_config::is_allowed_sidecar_name("evil-box"));
        assert!(!crate::core_config::is_allowed_sidecar_name("bash"));
        assert!(!crate::core_config::is_allowed_sidecar_name("sing-box.exe"));
    }
}
