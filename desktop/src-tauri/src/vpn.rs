use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter};

use crate::core_config::{self, check_core_config};
use crate::escape::{self, supervisor_launch_shell};
use crate::fsutil;
use crate::identity::{self, ProcessClaim};
use crate::privilege;
use crate::profile::{self, Profile, ProfileView};
use crate::redact::{self, Redactor};
use crate::status::{VpnState, VpnStatus};
use crate::supervisor;

const READY_TIMEOUT: Duration = Duration::from_secs(15);
const STOP_WAIT: Duration = Duration::from_secs(8);
const READY_MARK: &str = "sing-box started (";

#[derive(Clone)]
pub struct AppState {
    shared: Arc<Shared>,
}

struct Shared {
    inner: Mutex<Inner>,
    op: Mutex<()>,
    cancel: AtomicBool,
}

struct Inner {
    status: VpnStatus,
    data_dir: PathBuf,
    session: Option<Running>,
    heartbeat_stop: Option<Arc<AtomicBool>>,
    heartbeat: Option<JoinHandle<()>>,
    last_log: Option<PathBuf>,
    last_redactor: Redactor,
    handle: Option<AppHandle>,
}

struct Running {
    dir: PathBuf,
    nonce: String,
    config_path: PathBuf,
    log_path: PathBuf,
    supervisor: ProcessClaim,
    singbox: Option<ProcessClaim>,
    redactor: Redactor,
    /// Owns the process-wide lock for as long as this session exists.
    #[allow(dead_code)]
    lock: SessionLock,
}

struct SessionLock {
    file: File,
}

#[derive(Serialize)]
pub struct LogView {
    pub lines: Vec<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ActiveFile {
    nonce: String,
    dir: String,
    config_path: String,
    log_path: String,
    supervisor_pid: u32,
    supervisor_start: u64,
    supervisor_exe: String,
    singbox_pid: Option<u32>,
    singbox_start: Option<u64>,
    singbox_exe: Option<String>,
}

impl AppState {
    pub fn new() -> Self {
        let data_dir = data_dir().unwrap_or_else(|_| PathBuf::from("/tmp/mieru-desktop-nohome"));
        Self {
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner {
                    status: VpnStatus::new(VpnState::Disconnected, ""),
                    data_dir,
                    session: None,
                    heartbeat_stop: None,
                    heartbeat: None,
                    last_log: None,
                    last_redactor: Redactor::from_secrets("", None),
                    handle: None,
                }),
                op: Mutex::new(()),
                cancel: AtomicBool::new(false),
            }),
        }
    }
}

impl SessionLock {
    fn try_acquire(path: &Path) -> Result<Self, String> {
        let file = fsutil::touch_private(path)
            .map_err(|_| "could not lock the managed core".to_string())?;
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EWOULDBLOCK)
                || err.raw_os_error() == Some(libc::EAGAIN)
            {
                return Err("a managed core is already running".into());
            }
            return Err("could not lock the managed core".into());
        }
        Ok(Self { file })
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

pub fn attach(app: &AppHandle, state: &AppState) {
    {
        let mut inner = lock_inner(&state.shared);
        inner.handle = Some(app.clone());
    }
    recover_stale(state);
}

pub fn shutdown_for_exit(state: &AppState) {
    state.shared.cancel.store(true, Ordering::SeqCst);
    if let Some(dir) = session_dir(state) {
        let _ = fs::write(dir.join("stop"), b"1");
    }
    let _op = lock_op(&state.shared);
    let _ = teardown(state, false, None);
}

pub fn profile_get(state: &AppState) -> Result<Option<ProfileView>, String> {
    let path = profile_path(state)?;
    Ok(profile::load(&path)?.map(|profile| profile.view()))
}

/// Load the saved profile for in-process checks.
/// The password stays in this value and must not reach the webview, logs, or argv.
pub(crate) fn load_saved_profile(state: &AppState) -> Result<Option<Profile>, String> {
    profile::load(&profile_path(state)?)
}

pub fn profile_set(state: &AppState, profile: Profile) -> Result<ProfileView, String> {
    profile.validate()?;
    let path = profile_path(state)?;
    profile::store(&path, &profile)?;
    Ok(profile.view())
}

pub fn vpn_status_get(state: &AppState) -> Result<VpnStatus, String> {
    Ok(lock_inner(&state.shared).status.clone())
}

pub fn vpn_log_get(state: &AppState) -> Result<LogView, String> {
    let inner = lock_inner(&state.shared);
    let path = inner
        .session
        .as_ref()
        .map(|session| session.log_path.clone())
        .or_else(|| inner.last_log.clone());
    let redactor = inner
        .session
        .as_ref()
        .map(|session| session.redactor.clone())
        .unwrap_or_else(|| inner.last_redactor.clone());
    drop(inner);
    let Some(path) = path else {
        return Ok(LogView { lines: Vec::new() });
    };
    let text = fsutil::read_tail(&path, 64 * 1024).unwrap_or_default();
    let cleaned = redactor.apply(&text);
    let mut lines: Vec<String> = cleaned
        .lines()
        .map(|line| redact::truncate(line.trim_end(), 500))
        .filter(|line| !line.is_empty())
        .collect();
    if lines.len() > 200 {
        lines = lines.split_off(lines.len() - 200);
    }
    Ok(LogView { lines })
}

pub fn vpn_connect(state: &AppState) -> Result<VpnStatus, String> {
    let _op = lock_op(&state.shared);
    if state.shared.cancel.swap(false, Ordering::SeqCst) {
        return finish_cancelled(state);
    }
    if unsafe { libc::getuid() } == 0 {
        return fail_status(state, "the desktop app must not run as root");
    }
    let data_dir = ensure_data_dir(state)?;
    let profile = profile::load(&data_dir.join("profile.json"))?
        .ok_or_else(|| publish_error(state, "profile is not set"))?;
    profile.validate()?;
    if lock_inner(&state.shared).session.is_some() {
        return Err("a managed core is already running".into());
    }
    let lock = SessionLock::try_acquire(&data_dir.join("instance.lock"))?;
    let lock = take_live_active(state, &data_dir, lock)?;
    publish(state, VpnStatus::new(VpnState::Connecting, ""));

    let redactor = Redactor::from_secrets(&profile.password, profile.traffic_pattern.as_deref());
    let server_ip = match core_config::resolve_server(&profile.server) {
        Ok(ip) => ip,
        Err(err) => return fail_locked(state, lock, redactor, err),
    };
    let mixed_port = match core_config::pick_mixed_port() {
        Ok(port) => port,
        Err(err) => return fail_locked(state, lock, redactor, err),
    };
    let config_body = match core_config::build_core_config(&profile, server_ip, mixed_port) {
        Ok(body) => body,
        Err(err) => return fail_locked(state, lock, redactor, err),
    };
    let sidecar = match core_config::sidecar_path() {
        Ok(path) => path,
        Err(err) => return fail_locked(state, lock, redactor, err),
    };
    let app_exe = match std::env::current_exe().and_then(|path| path.canonicalize()) {
        Ok(path) => path,
        Err(_) => {
            return fail_locked(
                state,
                lock,
                redactor,
                "could not resolve the desktop executable".into(),
            )
        }
    };

    let nonce = match new_nonce() {
        Ok(nonce) => nonce,
        Err(err) => return fail_locked(state, lock, redactor, err),
    };
    let dir = data_dir.join("sessions").join(&nonce);
    if let Err(err) = fsutil::ensure_dir(&dir) {
        let _ = err;
        return fail_locked(
            state,
            lock,
            redactor,
            "could not create the session directory".into(),
        );
    }
    let config_path = dir.join("config.json");
    let log_path = dir.join("core.log");
    let workdir = dir.join("workdir");
    if let Err(err) = write_session_files(&dir, &config_path, &log_path, &workdir, &config_body) {
        let _ = fs::remove_dir_all(&dir);
        return fail_locked(state, lock, redactor, err);
    }
    if let Err(err) = check_core_config(&sidecar, &config_path, &redactor) {
        let _ = fs::remove_file(&config_path);
        let _ = fs::remove_dir_all(&dir);
        return fail_locked(state, lock, redactor.clone(), err);
    }

    let owner_pid = std::process::id();
    let owner = match identity::inspect(owner_pid) {
        Some(facts) => facts,
        None => {
            let _ = fs::remove_dir_all(&dir);
            return fail_locked(
                state,
                lock,
                redactor,
                "could not inspect the desktop process".into(),
            );
        }
    };
    if !identity::same_file(&owner.exe, &app_exe) {
        let _ = fs::remove_dir_all(&dir);
        return fail_locked(
            state,
            lock,
            redactor,
            "could not confirm the desktop executable".into(),
        );
    }
    if let Err(err) = write_launch_file(
        &dir,
        &sidecar,
        &config_path,
        &workdir,
        &log_path,
        owner_pid,
        owner.start_sec,
        &nonce,
    ) {
        let _ = fs::remove_dir_all(&dir);
        return fail_locked(state, lock, redactor, err);
    }

    let heartbeat_stop = Arc::new(AtomicBool::new(false));
    let heartbeat = spawn_heartbeat(dir.join("heartbeat"), Arc::clone(&heartbeat_stop));
    {
        let mut inner = lock_inner(&state.shared);
        inner.heartbeat_stop = Some(Arc::clone(&heartbeat_stop));
        inner.heartbeat = Some(heartbeat);
        inner.last_redactor = redactor.clone();
        inner.last_log = Some(log_path.clone());
    }

    let runtime = match dir.to_str() {
        Some(path) => path.to_string(),
        None => {
            return abort_start(state, lock, &dir, redactor, "session path is not utf-8");
        }
    };
    let binary = match app_exe.to_str() {
        Some(path) => path.to_string(),
        None => return abort_start(state, lock, &dir, redactor, "executable path is not utf-8"),
    };
    let shell = match supervisor_launch_shell(&binary, &runtime, owner_pid) {
        Ok(shell) => shell,
        Err(err) => return abort_start(state, lock, &dir, redactor, &err),
    };
    if shell.contains(&profile.password)
        || profile
            .traffic_pattern
            .as_ref()
            .is_some_and(|p| shell.contains(p))
    {
        return abort_start(
            state,
            lock,
            &dir,
            redactor,
            "refusing to pass profile secrets to the elevated command",
        );
    }

    let elevated = match privilege::elevate(&shell) {
        Ok(stdout) => stdout,
        Err(err) => return abort_start(state, lock, &dir, redactor, &err),
    };
    let reported_pid = elevated.trim().parse::<u32>().ok().filter(|pid| *pid >= 2);
    if state.shared.cancel.load(Ordering::SeqCst) {
        return abort_elevated(
            state,
            lock,
            &dir,
            &app_exe,
            &nonce,
            reported_pid,
            redactor,
            None,
        );
    }
    let Some(supervisor_pid) = reported_pid else {
        return abort_elevated(
            state,
            lock,
            &dir,
            &app_exe,
            &nonce,
            None,
            redactor,
            Some("the elevated core did not report a pid"),
        );
    };
    let supervisor_facts = match wait_supervisor(
        &dir,
        &app_exe,
        supervisor_pid,
        &nonce,
        Duration::from_secs(5),
    ) {
        Some(facts) => facts,
        None => {
            return abort_elevated(
                state,
                lock,
                &dir,
                &app_exe,
                &nonce,
                Some(supervisor_pid),
                redactor,
                Some("could not confirm the elevated process"),
            );
        }
    };
    let supervisor = ProcessClaim {
        pid: supervisor_pid,
        exe: app_exe,
        start_sec: supervisor_facts.start_sec,
        marker: nonce.clone(),
        dir: Some(dir.clone()),
    };
    let singbox = wait_child_claim(&dir, Duration::from_secs(3));
    let running = Running {
        dir: dir.clone(),
        nonce: nonce.clone(),
        config_path: config_path.clone(),
        log_path: log_path.clone(),
        supervisor: supervisor.clone(),
        singbox,
        redactor: redactor.clone(),
        lock,
    };
    if let Err(err) = write_active(&data_dir, &running) {
        lock_inner(&state.shared).session = Some(running);
        return abort_running(state, redactor, &err);
    }
    lock_inner(&state.shared).session = Some(running);

    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        if state.shared.cancel.load(Ordering::SeqCst) {
            let _ = teardown(
                state,
                true,
                Some(VpnStatus::new(VpnState::Disconnected, "")),
            );
            return Ok(current_status(state));
        }
        if log_ready(&log_path) {
            if let Some(claim) = supervisor::child_claim(&dir) {
                if let Some(session) = lock_inner(&state.shared).session.as_mut() {
                    session.singbox = Some(claim);
                }
            }
            let _ = fs::remove_file(&config_path);
            let status = VpnStatus::new(VpnState::Connected, "");
            publish(state, status.clone());
            spawn_monitor(Arc::clone(&state.shared), nonce);
            return Ok(status);
        }
        if !identity::claim_matches(&supervisor) {
            let detail = redactor.last_line(&read_log(&log_path), 240);
            let message = if detail.is_empty() {
                "core exited before it was ready".to_string()
            } else {
                format!("core exited before it was ready: {detail}")
            };
            return abort_running(state, redactor, &message);
        }
        if Instant::now() >= deadline {
            return abort_running(
                state,
                redactor,
                "core did not become ready within 15 seconds",
            );
        }
        thread::sleep(Duration::from_millis(200));
    }
}

pub fn vpn_disconnect(state: &AppState) -> Result<VpnStatus, String> {
    state.shared.cancel.store(true, Ordering::SeqCst);
    if let Some(dir) = session_dir(state) {
        let _ = fs::write(dir.join("stop"), b"1");
        publish(state, VpnStatus::new(VpnState::Disconnecting, ""));
    }
    let _op = lock_op(&state.shared);
    state.shared.cancel.store(false, Ordering::SeqCst);
    let status = if lock_inner(&state.shared).session.is_some() {
        teardown(
            state,
            true,
            Some(VpnStatus::new(VpnState::Disconnected, "")),
        )?
    } else {
        clear_active_file(&lock_inner(&state.shared).data_dir);
        publish(state, VpnStatus::new(VpnState::Disconnected, ""));
        current_status(state)
    };
    if status.state == VpnState::Error {
        return Err(if status.message.is_empty() {
            "could not stop the managed core".to_string()
        } else {
            status.message
        });
    }
    Ok(status)
}

fn finish_cancelled(state: &AppState) -> Result<VpnStatus, String> {
    if lock_inner(&state.shared).session.is_some() {
        teardown(
            state,
            true,
            Some(VpnStatus::new(VpnState::Disconnected, "")),
        )
    } else {
        publish(state, VpnStatus::new(VpnState::Disconnected, ""));
        Ok(current_status(state))
    }
}

fn fail_status(state: &AppState, message: &str) -> Result<VpnStatus, String> {
    Err(publish_error(state, message))
}

fn fail_locked(
    state: &AppState,
    lock: SessionLock,
    redactor: Redactor,
    message: String,
) -> Result<VpnStatus, String> {
    drop(lock);
    {
        let mut inner = lock_inner(&state.shared);
        inner.last_redactor = redactor;
    }
    Err(publish_error(
        state,
        &redact_message(&state.shared, &message),
    ))
}

fn abort_start(
    state: &AppState,
    lock: SessionLock,
    dir: &Path,
    redactor: Redactor,
    message: &str,
) -> Result<VpnStatus, String> {
    stop_heartbeat(state);
    let _ = fs::remove_file(dir.join("config.json"));
    drop(lock);
    {
        let mut inner = lock_inner(&state.shared);
        inner.last_redactor = redactor.clone();
        inner.last_log = Some(dir.join("core.log"));
    }
    Err(publish_error(state, &redactor.apply(message)))
}

fn abort_elevated(
    state: &AppState,
    lock: SessionLock,
    dir: &Path,
    exe: &Path,
    nonce: &str,
    pid: Option<u32>,
    redactor: Redactor,
    message: Option<&str>,
) -> Result<VpnStatus, String> {
    stop_heartbeat(state);
    let _ = fs::write(dir.join("stop"), b"1");
    let _ = fs::remove_file(dir.join("config.json"));
    let deadline = Instant::now() + Duration::from_secs(12);
    while Instant::now() < deadline {
        if pid.is_some_and(|pid| !process_is_supervisor(pid, exe, nonce)) {
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    if let Some(pid) = pid {
        if let Some(claim) = supervisor_claim_if_ours(pid, exe, nonce) {
            let running = Running {
                dir: dir.to_path_buf(),
                nonce: nonce.to_string(),
                config_path: dir.join("config.json"),
                log_path: dir.join("core.log"),
                supervisor: claim,
                singbox: supervisor::child_claim(dir),
                redactor: redactor.clone(),
                lock,
            };
            lock_inner(&state.shared).session = Some(running);
            let final_status = match message {
                None => Some(VpnStatus::new(VpnState::Disconnected, "")),
                Some(message) => Some(VpnStatus::new(VpnState::Error, redactor.apply(message))),
            };
            let status = teardown(state, true, final_status)?;
            return match message {
                None => Ok(status),
                Some(message) => Err(redactor.apply(message)),
            };
        }
    }
    drop(lock);
    {
        let mut inner = lock_inner(&state.shared);
        inner.last_redactor = redactor.clone();
        inner.last_log = Some(dir.join("core.log"));
    }
    match message {
        None => {
            publish(state, VpnStatus::new(VpnState::Disconnected, ""));
            Ok(current_status(state))
        }
        Some(message) => Err(publish_error(state, &redactor.apply(message))),
    }
}

fn process_is_supervisor(pid: u32, exe: &Path, nonce: &str) -> bool {
    supervisor_claim_if_ours(pid, exe, nonce).is_some_and(|claim| identity::claim_matches(&claim))
}

fn supervisor_claim_if_ours(pid: u32, exe: &Path, nonce: &str) -> Option<ProcessClaim> {
    let facts = identity::inspect(pid)?;
    if identity::same_file(&facts.exe, exe)
        && facts.args.contains("--mieru-supervisor")
        && facts.args.contains(nonce)
    {
        Some(ProcessClaim {
            pid,
            exe: exe.to_path_buf(),
            start_sec: facts.start_sec,
            marker: nonce.to_string(),
            dir: None,
        })
    } else {
        None
    }
}

fn take_live_active(
    state: &AppState,
    data_dir: &Path,
    lock: SessionLock,
) -> Result<SessionLock, String> {
    let Some(active) = read_active(data_dir) else {
        return Ok(lock);
    };
    let supervisor = supervisor_claim(&active);
    let singbox = singbox_claim(&active);
    let alive = identity::claim_matches(&supervisor)
        || singbox.as_ref().is_some_and(identity::claim_matches);
    if !alive {
        let _ = fs::remove_file(Path::new(&active.config_path));
        clear_active_file(data_dir);
        return Ok(lock);
    }
    let redactor = profile_redactor(data_dir);
    let running = Running {
        dir: PathBuf::from(&active.dir),
        nonce: active.nonce,
        config_path: PathBuf::from(&active.config_path),
        log_path: PathBuf::from(&active.log_path),
        supervisor,
        singbox,
        redactor,
        lock,
    };
    {
        let mut inner = lock_inner(&state.shared);
        inner.last_log = Some(running.log_path.clone());
        inner.last_redactor = running.redactor.clone();
        inner.session = Some(running);
    }
    Err(publish_error(state, "a managed core is already running"))
}

fn abort_running(state: &AppState, redactor: Redactor, message: &str) -> Result<VpnStatus, String> {
    let message = redactor.apply(message);
    let _ = teardown(
        state,
        true,
        Some(VpnStatus::new(VpnState::Error, message.clone())),
    );
    Err(message)
}

fn teardown(
    state: &AppState,
    allow_elevate: bool,
    final_status: Option<VpnStatus>,
) -> Result<VpnStatus, String> {
    let session = {
        let mut inner = lock_inner(&state.shared);
        inner.session.take()
    };
    stop_heartbeat(state);
    let Some(session) = session else {
        let status = final_status.unwrap_or_else(|| VpnStatus::new(VpnState::Disconnected, ""));
        publish(state, status.clone());
        return Ok(status);
    };
    let _ = fs::write(session.dir.join("stop"), b"1");
    let _ = fs::remove_file(&session.config_path);
    let supervisor_gone = wait_until(STOP_WAIT, || !identity::claim_matches(&session.supervisor));
    if !supervisor_gone && allow_elevate {
        let _ = signal_owned(&session.supervisor, "TERM");
        let _ = wait_until(Duration::from_secs(5), || {
            !identity::claim_matches(&session.supervisor)
        });
        if identity::claim_matches(&session.supervisor) {
            let _ = signal_owned(&session.supervisor, "KILL");
        }
    }
    if let Some(singbox) = &session.singbox {
        if identity::claim_matches(singbox) && allow_elevate {
            let _ = signal_owned(singbox, "TERM");
            let _ = wait_until(Duration::from_secs(5), || !identity::claim_matches(singbox));
            if identity::claim_matches(singbox) {
                let _ = signal_owned(singbox, "KILL");
            }
        }
    }
    let stuck = identity::claim_matches(&session.supervisor)
        || session
            .singbox
            .as_ref()
            .is_some_and(identity::claim_matches);
    let mut status = final_status.unwrap_or_else(|| VpnStatus::new(VpnState::Disconnected, ""));
    if stuck && status.state != VpnState::Error {
        status = VpnStatus::new(VpnState::Error, "could not stop the managed core");
    }
    {
        let mut inner = lock_inner(&state.shared);
        inner.last_log = Some(session.log_path.clone());
        inner.last_redactor = session.redactor.clone();
        if stuck {
            let _ = write_active(&inner.data_dir, &session);
            inner.session = Some(session);
        } else {
            clear_active_file(&inner.data_dir);
            drop(session);
        }
    }
    publish(state, status.clone());
    Ok(status)
}

fn signal_owned(claim: &ProcessClaim, signal: &str) -> Result<(), String> {
    if !identity::claim_matches(claim) {
        return Ok(());
    }
    let shell = escape::kill_shell(claim.pid, signal)?;
    privilege::elevate(&shell).map(|_| ())
}

fn recover_stale(state: &AppState) {
    let Ok(_op) = state.shared.op.lock() else {
        return;
    };
    let data_dir = match ensure_data_dir(state) {
        Ok(dir) => dir,
        Err(_) => return,
    };
    let Some(active) = read_active(&data_dir) else {
        return;
    };
    let supervisor = supervisor_claim(&active);
    let singbox = singbox_claim(&active);
    let supervisor_alive = identity::claim_matches(&supervisor);
    let singbox_alive = singbox.as_ref().is_some_and(identity::claim_matches);
    if !supervisor_alive && !singbox_alive {
        let _ = fs::remove_file(Path::new(&active.config_path));
        clear_active_file(&data_dir);
        return;
    }
    let Ok(lock) = SessionLock::try_acquire(&data_dir.join("instance.lock")) else {
        publish_error(state, "a managed core is already running");
        return;
    };
    let redactor = profile_redactor(&data_dir);
    let running = Running {
        dir: PathBuf::from(&active.dir),
        nonce: active.nonce.clone(),
        config_path: PathBuf::from(&active.config_path),
        log_path: PathBuf::from(&active.log_path),
        supervisor,
        singbox,
        redactor,
        lock,
    };
    {
        let mut inner = lock_inner(&state.shared);
        inner.last_log = Some(running.log_path.clone());
        inner.session = Some(running);
    }
    let _ = teardown(
        state,
        false,
        Some(VpnStatus::new(VpnState::Disconnected, "")),
    );
    if lock_inner(&state.shared)
        .session
        .as_ref()
        .is_some_and(|session| identity::claim_matches(&session.supervisor))
    {
        publish_error(state, "a managed core is still running");
    }
}

fn spawn_monitor(shared: Arc<Shared>, nonce: String) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(500));
        let alive = {
            let inner = lock_inner(&shared);
            let Some(session) = inner.session.as_ref() else {
                return;
            };
            if session.nonce != nonce || inner.status.state != VpnState::Connected {
                return;
            }
            identity::claim_matches(&session.supervisor)
        };
        if alive {
            continue;
        }
        let Ok(_op) = shared.op.lock() else {
            return;
        };
        let still = {
            let inner = lock_inner(&shared);
            inner
                .session
                .as_ref()
                .is_some_and(|session| session.nonce == nonce)
                && inner.status.state == VpnState::Connected
        };
        if !still {
            return;
        }
        let state = AppState {
            shared: Arc::clone(&shared),
        };
        let detail = {
            let inner = lock_inner(&shared);
            let session = inner.session.as_ref();
            session
                .map(|session| {
                    session
                        .redactor
                        .last_line(&read_log(&session.log_path), 240)
                })
                .unwrap_or_default()
        };
        let message = if detail.is_empty() {
            "core exited unexpectedly".to_string()
        } else {
            format!("core exited unexpectedly: {detail}")
        };
        let _ = teardown(&state, true, Some(VpnStatus::new(VpnState::Error, message)));
        return;
    });
}

fn spawn_heartbeat(path: PathBuf, stop: Arc<AtomicBool>) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut beat = 1u64;
        while !stop.load(Ordering::Relaxed) {
            if let Ok(mut file) = OpenOptions::new().write(true).truncate(true).open(&path) {
                let _ = write!(file, "{beat}");
            }
            beat = beat.wrapping_add(1);
            for _ in 0..20 {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
    })
}

fn write_session_files(
    dir: &Path,
    config_path: &Path,
    log_path: &Path,
    workdir: &Path,
    config_body: &str,
) -> Result<(), String> {
    fsutil::ensure_dir(dir).map_err(|_| "could not create the session directory".to_string())?;
    fsutil::ensure_dir(workdir)
        .map_err(|_| "could not create the core work directory".to_string())?;
    fsutil::write_private(config_path, config_body.as_bytes())
        .map_err(|_| "could not store the core configuration".to_string())?;
    fsutil::touch_private(log_path).map_err(|_| "could not create the core log".to_string())?;
    fsutil::touch_private(&dir.join("supervisor.log"))
        .map_err(|_| "could not create the supervisor log".to_string())?;
    fsutil::touch_private(&dir.join("state.json"))
        .map_err(|_| "could not create the state file".to_string())?;
    fsutil::touch_private(&dir.join("heartbeat"))
        .map_err(|_| "could not create the heartbeat file".to_string())?;
    Ok(())
}

fn write_launch_file(
    dir: &Path,
    sidecar: &Path,
    config_path: &Path,
    workdir: &Path,
    log_path: &Path,
    owner_pid: u32,
    owner_start_sec: u64,
    nonce: &str,
) -> Result<(), String> {
    let launch = json!({
        "sing_box": path_string(sidecar)?,
        "config": path_string(config_path)?,
        "workdir": path_string(workdir)?,
        "log_path": path_string(log_path)?,
        "owner_pid": owner_pid,
        "owner_start_sec": owner_start_sec,
        "owner_uid": unsafe { libc::getuid() },
        "owner_gid": unsafe { libc::getgid() },
        "nonce": nonce,
    });
    let bytes = serde_json::to_vec_pretty(&launch)
        .map_err(|_| "could not store the launch file".to_string())?;
    fsutil::write_private(&dir.join("launch.json"), &bytes)
        .map_err(|_| "could not store the launch file".to_string())
}

fn write_active(data_dir: &Path, running: &Running) -> Result<(), String> {
    let active = ActiveFile {
        nonce: running.nonce.clone(),
        dir: path_string(&running.dir)?,
        config_path: path_string(&running.config_path)?,
        log_path: path_string(&running.log_path)?,
        supervisor_pid: running.supervisor.pid,
        supervisor_start: running.supervisor.start_sec,
        supervisor_exe: path_string(&running.supervisor.exe)?,
        singbox_pid: running.singbox.as_ref().map(|claim| claim.pid),
        singbox_start: running.singbox.as_ref().map(|claim| claim.start_sec),
        singbox_exe: running
            .singbox
            .as_ref()
            .map(|claim| path_string(&claim.exe))
            .transpose()?,
    };
    let bytes = serde_json::to_vec_pretty(&active)
        .map_err(|_| "could not store the active session".to_string())?;
    fsutil::write_private(&data_dir.join("active.json"), &bytes)
        .map_err(|_| "could not store the active session".to_string())
}

fn wait_supervisor(
    dir: &Path,
    exe: &Path,
    pid: u32,
    nonce: &str,
    timeout: Duration,
) -> Option<identity::ProcessFacts> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(facts) = identity::inspect(pid) {
            if identity::same_file(&facts.exe, exe) {
                let has_nonce =
                    facts.args.contains("--mieru-supervisor") && facts.args.contains(nonce);
                let has_handshake = supervisor::confirm_handshake(dir, pid, nonce);
                if has_nonce || has_handshake {
                    return Some(facts);
                }
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_child_claim(dir: &Path, timeout: Duration) -> Option<ProcessClaim> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(claim) = supervisor::child_claim(dir) {
            return Some(claim);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_until(timeout: Duration, mut pred: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if pred() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn log_ready(path: &Path) -> bool {
    read_log(path).contains(READY_MARK)
}

fn read_log(path: &Path) -> String {
    fsutil::read_tail(path, 256 * 1024).unwrap_or_default()
}

fn stop_heartbeat(state: &AppState) {
    let mut inner = lock_inner(&state.shared);
    if let Some(flag) = inner.heartbeat_stop.take() {
        flag.store(true, Ordering::SeqCst);
    }
    if let Some(handle) = inner.heartbeat.take() {
        drop(inner);
        let _ = handle.join();
    }
}

fn publish(state: &AppState, status: VpnStatus) {
    let handle = {
        let mut inner = lock_inner(&state.shared);
        inner.status = status.clone();
        inner.handle.clone()
    };
    if let Some(handle) = handle {
        let _ = handle.emit("vpn-status", &status);
    }
}

fn publish_error(state: &AppState, message: &str) -> String {
    let message = redact_message(&state.shared, message);
    publish(state, VpnStatus::new(VpnState::Error, message.clone()));
    message
}

fn redact_message(shared: &Shared, message: &str) -> String {
    let inner = lock_inner(shared);
    let redacted = if let Some(session) = inner.session.as_ref() {
        session.redactor.apply(message)
    } else {
        inner.last_redactor.apply(message)
    };
    redact::truncate(&redacted, 300)
}

fn current_status(state: &AppState) -> VpnStatus {
    lock_inner(&state.shared).status.clone()
}

fn session_dir(state: &AppState) -> Option<PathBuf> {
    lock_inner(&state.shared)
        .session
        .as_ref()
        .map(|session| session.dir.clone())
}

fn profile_path(state: &AppState) -> Result<PathBuf, String> {
    Ok(ensure_data_dir(state)?.join("profile.json"))
}

fn ensure_data_dir(state: &AppState) -> Result<PathBuf, String> {
    let dir = data_dir()?;
    fsutil::ensure_dir(&dir)
        .map_err(|_| "could not create the application directory".to_string())?;
    fsutil::ensure_dir(&dir.join("sessions"))
        .map_err(|_| "could not create the application directory".to_string())?;
    lock_inner(&state.shared).data_dir = dir.clone();
    Ok(dir)
}

fn data_dir() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    let home = PathBuf::from(home);
    if !home.is_absolute() {
        return Err("HOME is not an absolute path".into());
    }
    Ok(home.join("Library/Application Support/Mieru"))
}

fn new_nonce() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    let mut file =
        File::open("/dev/urandom").map_err(|_| "could not generate a session id".to_string())?;
    file.read_exact(&mut bytes)
        .map_err(|_| "could not generate a session id".to_string())?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn path_string(path: &Path) -> Result<String, String> {
    path.to_str()
        .filter(|text| !text.chars().any(|ch| ch.is_control()))
        .map(str::to_string)
        .ok_or_else(|| "a path cannot be passed to the elevated command".to_string())
}

fn clear_active_file(data_dir: &Path) {
    let _ = fs::remove_file(data_dir.join("active.json"));
}

fn read_active(data_dir: &Path) -> Option<ActiveFile> {
    let bytes = fs::read(data_dir.join("active.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn supervisor_claim(active: &ActiveFile) -> ProcessClaim {
    ProcessClaim {
        pid: active.supervisor_pid,
        exe: PathBuf::from(&active.supervisor_exe),
        start_sec: active.supervisor_start,
        marker: active.nonce.clone(),
        dir: Some(PathBuf::from(&active.dir)),
    }
}

fn singbox_claim(active: &ActiveFile) -> Option<ProcessClaim> {
    Some(ProcessClaim {
        pid: active.singbox_pid?,
        exe: PathBuf::from(active.singbox_exe.clone()?),
        start_sec: active.singbox_start?,
        marker: active.config_path.clone(),
        dir: Some(PathBuf::from(&active.dir)),
    })
}

fn profile_redactor(data_dir: &Path) -> Redactor {
    match profile::load(&data_dir.join("profile.json")) {
        Ok(Some(profile)) => {
            Redactor::from_secrets(&profile.password, profile.traffic_pattern.as_deref())
        }
        _ => Redactor::from_secrets("", None),
    }
}

fn lock_inner(shared: &Shared) -> std::sync::MutexGuard<'_, Inner> {
    shared.inner.lock().unwrap_or_else(|err| err.into_inner())
}

fn lock_op(shared: &Shared) -> std::sync::MutexGuard<'_, ()> {
    shared.op.lock().unwrap_or_else(|err| err.into_inner())
}
