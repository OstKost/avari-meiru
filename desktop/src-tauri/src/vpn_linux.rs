use std::fs::{self, File};
use std::io::{ErrorKind, Read};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::fsutil;
use crate::profile::{self, Profile, ProfileView};
use crate::redact::{self, Redactor};
use crate::status::{VpnState, VpnStatus};

use mieru_shared::install::{self, HELPER_PATH, PKEXEC_PATH, SIDECAR_PATH};
use mieru_shared::procinfo;
use mieru_shared::protocol::{self, ClientMsg, ServerMsg};

const STATUS_WAIT: Duration = Duration::from_secs(45);

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
    nonce: String,
    dir: PathBuf,
    log_path: PathBuf,
    helper_pid: u32,
    helper_start: u64,
    writer: Arc<Mutex<UnixStream>>,
    redactor: Redactor,
    pkexec: Child,
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
            return Err("a managed core is already running".into());
        }
        Ok(Self { file })
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
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
    let _op = lock_op(&state.shared);
    let _ = teardown(state, Some(VpnStatus::new(VpnState::Disconnected, "")));
}

pub fn profile_get(state: &AppState) -> Result<Option<ProfileView>, String> {
    let path = profile_path(state)?;
    Ok(profile::load(&path)?.map(|profile| profile.view()))
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
        return Err(publish_error(state, "the desktop app must not run as root"));
    }
    let data_dir = ensure_data_dir(state)?;
    let profile = profile::load(&data_dir.join("profile.json"))?
        .ok_or_else(|| publish_error(state, "profile is not set"))?;
    profile.validate()?;
    if lock_inner(&state.shared).session.is_some() {
        return Err("a managed core is already running".into());
    }
    let lock = SessionLock::try_acquire(&data_dir.join("instance.lock"))?;
    let redactor = Redactor::from_secrets(&profile.password, profile.traffic_pattern.as_deref());
    if let Err(err) = ensure_installed() {
        drop(lock);
        return Err(publish_error(state, &redactor.apply(&err)));
    }
    publish(state, VpnStatus::new(VpnState::Connecting, ""));
    let nonce = new_nonce().map_err(|err| publish_error(state, &err))?;
    let uid = unsafe { libc::getuid() };
    let dir = match install::runtime_session_dir(uid, &nonce) {
        Ok(dir) => dir,
        Err(err) => return fail_locked(state, lock, redactor, err),
    };
    if !Path::new(&format!("/run/user/{uid}")).is_dir() {
        return fail_locked(
            state,
            lock,
            redactor,
            "the user runtime directory is missing".into(),
        );
    }
    if let Err(err) = fsutil::ensure_dir(&dir) {
        let _ = err;
        return fail_locked(
            state,
            lock,
            redactor,
            "could not create the session directory".into(),
        );
    }
    let args = match install::helper_args(&nonce, std::process::id()) {
        Ok(args) => args,
        Err(err) => return fail_locked(state, lock, redactor, err),
    };
    let joined = args.join(" ");
    if joined.contains(&profile.password)
        || profile
            .traffic_pattern
            .as_ref()
            .is_some_and(|p| joined.contains(p))
    {
        return fail_locked(
            state,
            lock,
            redactor,
            "refusing to pass profile secrets to the elevated command".into(),
        );
    }
    let socket_path = dir.join("helper.sock");
    let log_path = dir.join("core.log");
    let mut command = Command::new(PKEXEC_PATH);
    command.arg(HELPER_PATH);
    for arg in &args {
        command.arg(arg);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            return fail_locked(
                state,
                lock,
                redactor,
                "could not start the elevated core".into(),
            )
        }
    };
    let stderr_buf = Arc::new(Mutex::new(String::new()));
    if let Some(stderr) = child.stderr.take() {
        let buf = Arc::clone(&stderr_buf);
        thread::spawn(move || {
            let mut reader = stderr;
            let mut tmp = [0u8; 1024];
            loop {
                match reader.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut guard = buf.lock().unwrap_or_else(|err| err.into_inner());
                        if guard.len() < 4096 {
                            guard.push_str(&String::from_utf8_lossy(&tmp[..n]));
                        }
                    }
                }
            }
        });
    }
    let stream = match wait_socket(
        &socket_path,
        &mut child,
        &state.shared,
        Duration::from_secs(180),
    ) {
        Ok(stream) => stream,
        Err(err) => {
            stop_pkexec(&mut child);
            let stderr = stderr_buf.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let message = if state.shared.cancel.load(Ordering::SeqCst) {
                String::new()
            } else if err == "cancelled" {
                String::new()
            } else {
                let code = child
                    .try_wait()
                    .ok()
                    .flatten()
                    .and_then(|status| status.code());
                safe_failure(code, &stderr, &redactor)
            };
            drop(lock);
            if message.is_empty() {
                publish(state, VpnStatus::new(VpnState::Disconnected, ""));
                return Ok(current_status(state));
            }
            return Err(publish_error(state, &message));
        }
    };
    if state.shared.cancel.load(Ordering::SeqCst) {
        let _ = protocol::write_frame(&mut &stream, &ClientMsg::Disconnect);
        stop_pkexec(&mut child);
        drop(lock);
        publish(state, VpnStatus::new(VpnState::Disconnected, ""));
        return Ok(current_status(state));
    }
    if protocol::write_frame(
        &mut &stream,
        &ClientMsg::Hello {
            session: nonce.clone(),
        },
    )
    .is_err()
    {
        stop_pkexec(&mut child);
        return fail_locked(
            state,
            lock,
            redactor,
            "could not talk to the elevated core".into(),
        );
    }
    let hello = match read_server_timeout(&stream, Duration::from_secs(5)) {
        Ok(ServerMsg::Hello { pid, start }) => (pid, start),
        _ => {
            stop_pkexec(&mut child);
            return fail_locked(
                state,
                lock,
                redactor,
                "could not confirm the elevated process".into(),
            );
        }
    };
    if !socket_peer_is_root_pid(&stream, hello.0)
        || !helper_identity(hello.0, hello.1, &nonce, std::process::id())
    {
        let _ = protocol::write_frame(&mut &stream, &ClientMsg::Disconnect);
        stop_pkexec(&mut child);
        return fail_locked(
            state,
            lock,
            redactor,
            "could not confirm the elevated process".into(),
        );
    }
    if protocol::write_frame(
        &mut &stream,
        &ClientMsg::Connect {
            profile: profile.clone(),
        },
    )
    .is_err()
    {
        stop_pkexec(&mut child);
        return fail_locked(
            state,
            lock,
            redactor,
            "could not talk to the elevated core".into(),
        );
    }
    let status = match read_server_timeout(&stream, STATUS_WAIT) {
        Ok(ServerMsg::Status {
            state: name,
            message,
        }) => (name, message),
        _ => {
            let _ = protocol::write_frame(&mut &stream, &ClientMsg::Disconnect);
            stop_pkexec(&mut child);
            return fail_locked(
                state,
                lock,
                redactor,
                "core did not become ready within 15 seconds".into(),
            );
        }
    };
    if status.0 != "connected" {
        let message = if status.1.is_empty() {
            "could not start the elevated core".to_string()
        } else {
            redactor.apply(&status.1)
        };
        stop_pkexec(&mut child);
        drop(lock);
        if status.0 == "disconnected" {
            publish(state, VpnStatus::new(VpnState::Disconnected, ""));
            return Ok(current_status(state));
        }
        return Err(publish_error(state, &message));
    }
    let (reader, writer) = match stream.try_clone() {
        Ok(reader) => (reader, stream),
        Err(_) => {
            stop_pkexec(&mut child);
            return fail_locked(
                state,
                lock,
                redactor,
                "could not talk to the elevated core".into(),
            );
        }
    };
    let writer = Arc::new(Mutex::new(writer));
    let heartbeat_stop = Arc::new(AtomicBool::new(false));
    let heartbeat = spawn_heartbeat(Arc::clone(&writer), Arc::clone(&heartbeat_stop));
    let running = Running {
        nonce: nonce.clone(),
        dir,
        log_path: log_path.clone(),
        helper_pid: hello.0,
        helper_start: hello.1,
        writer: Arc::clone(&writer),
        redactor: redactor.clone(),
        pkexec: child,
        lock,
    };
    {
        let mut inner = lock_inner(&state.shared);
        inner.heartbeat_stop = Some(heartbeat_stop);
        inner.heartbeat = Some(heartbeat);
        inner.last_log = Some(log_path);
        inner.last_redactor = redactor;
        inner.session = Some(running);
    }
    let _ = write_active(&data_dir, hello.0, hello.1, &nonce);
    publish(state, VpnStatus::new(VpnState::Connected, ""));
    spawn_monitor(Arc::clone(&state.shared), nonce, reader);
    Ok(current_status(state))
}

pub fn vpn_disconnect(state: &AppState) -> Result<VpnStatus, String> {
    state.shared.cancel.store(true, Ordering::SeqCst);
    if lock_inner(&state.shared).session.is_some() {
        publish(state, VpnStatus::new(VpnState::Disconnecting, ""));
    }
    let _op = lock_op(&state.shared);
    state.shared.cancel.store(false, Ordering::SeqCst);
    let status = teardown(state, Some(VpnStatus::new(VpnState::Disconnected, "")))?;
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
        teardown(state, Some(VpnStatus::new(VpnState::Disconnected, "")))
    } else {
        publish(state, VpnStatus::new(VpnState::Disconnected, ""));
        Ok(current_status(state))
    }
}

fn fail_locked(
    state: &AppState,
    lock: SessionLock,
    redactor: Redactor,
    message: String,
) -> Result<VpnStatus, String> {
    drop(lock);
    lock_inner(&state.shared).last_redactor = redactor.clone();
    Err(publish_error(state, &redactor.apply(&message)))
}

fn teardown(state: &AppState, final_status: Option<VpnStatus>) -> Result<VpnStatus, String> {
    let session = lock_inner(&state.shared).session.take();
    stop_heartbeat(state);
    let Some(mut session) = session else {
        let status = final_status.unwrap_or_else(|| VpnStatus::new(VpnState::Disconnected, ""));
        publish(state, status.clone());
        return Ok(status);
    };
    if let Ok(mut writer) = session.writer.lock() {
        let _ = protocol::write_frame(&mut *writer, &ClientMsg::Disconnect);
    }
    let helper_pid = session.helper_pid;
    let helper_start = session.helper_start;
    let helper_gone = wait_owned_helper(
        &mut session.pkexec,
        helper_pid,
        helper_start,
        Duration::from_secs(12),
    );
    if !helper_gone {
        // Shut the socket down so the helper sees the GUI disconnect and cleans up.
        // The session value stays intact if the helper is still alive afterwards.
        if let Ok(stream) = session.writer.lock() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        let _ = wait_owned_helper(
            &mut session.pkexec,
            helper_pid,
            helper_start,
            Duration::from_secs(12),
        );
    }
    let still = !owned_helper_stopped(&mut session.pkexec, helper_pid, helper_start);
    stop_pkexec(&mut session.pkexec);
    let mut status = final_status.unwrap_or_else(|| VpnStatus::new(VpnState::Disconnected, ""));
    {
        let mut inner = lock_inner(&state.shared);
        inner.last_log = Some(session.log_path.clone());
        inner.last_redactor = session.redactor.clone();
        if still {
            status = VpnStatus::new(VpnState::Error, "could not stop the managed core");
            inner.session = Some(session);
        } else {
            clear_active(&inner.data_dir);
            let _ = fs::remove_dir_all(&session.dir);
            drop(session);
        }
    }
    publish(state, status.clone());
    Ok(status)
}

fn recover_stale(state: &AppState) {
    let Ok(_op) = state.shared.op.try_lock() else {
        return;
    };
    let data_dir = match ensure_data_dir(state) {
        Ok(dir) => dir,
        Err(_) => return,
    };
    let Some(active) = read_active(&data_dir) else {
        return;
    };
    if !wait_dead(
        active.helper_pid,
        active.helper_start,
        Duration::from_secs(12),
    ) && procinfo::process_alive(active.helper_pid, active.helper_start)
        && helper_identity(
            active.helper_pid,
            active.helper_start,
            &active.session,
            std::process::id(),
        )
    {
        publish_error(state, "a managed core is still running");
        return;
    }
    clear_active(&data_dir);
}

fn spawn_monitor(shared: Arc<Shared>, nonce: String, reader: UnixStream) {
    thread::spawn(move || loop {
        match read_server_event(&reader) {
            Ok(None) => continue,
            Ok(Some(ServerMsg::Status {
                state: name,
                message,
            })) if name == "error" => {
                let state = AppState {
                    shared: Arc::clone(&shared),
                };
                let _ = teardown(
                    &state,
                    Some(VpnStatus::new(
                        VpnState::Error,
                        redact_message(&shared, &message),
                    )),
                );
                return;
            }
            Ok(Some(ServerMsg::Status { state: name, .. })) if name == "disconnected" => {
                let state = AppState {
                    shared: Arc::clone(&shared),
                };
                if lock_inner(&shared)
                    .session
                    .as_ref()
                    .is_some_and(|session| session.nonce == nonce)
                {
                    let _ = teardown(&state, Some(VpnStatus::new(VpnState::Disconnected, "")));
                }
                return;
            }
            Ok(Some(_)) => {}
            Err(()) => {
                let state = AppState {
                    shared: Arc::clone(&shared),
                };
                let current = lock_inner(&shared).status.state;
                let same = lock_inner(&shared)
                    .session
                    .as_ref()
                    .is_some_and(|session| session.nonce == nonce);
                if same && current == VpnState::Connected {
                    let _ = teardown(
                        &state,
                        Some(VpnStatus::new(VpnState::Error, "core exited unexpectedly")),
                    );
                }
                return;
            }
        }
    });
}

fn spawn_heartbeat(writer: Arc<Mutex<UnixStream>>, stop: Arc<AtomicBool>) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut beat = 1u64;
        while !stop.load(Ordering::Relaxed) {
            if let Ok(mut stream) = writer.lock() {
                if protocol::write_frame(&mut *stream, &ClientMsg::Heartbeat { n: beat }).is_err() {
                    return;
                }
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

fn wait_socket(
    path: &Path,
    child: &mut Child,
    shared: &Shared,
    timeout: Duration,
) -> Result<UnixStream, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if shared.cancel.load(Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        if let Ok(Some(status)) = child.try_wait() {
            let _ = status;
            return Err("pkexec-exited".into());
        }
        if let Ok(stream) = UnixStream::connect(path) {
            return Ok(stream);
        }
        if Instant::now() >= deadline {
            return Err("the desktop did not connect".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn read_server_timeout(stream: &UnixStream, timeout: Duration) -> Result<ServerMsg, String> {
    let _ = stream.set_read_timeout(Some(timeout));
    let bytes = protocol::read_frame(stream).map_err(|_| "ipc message is invalid".to_string())?;
    protocol::decode_server(&bytes)
}

fn read_server_event(stream: &UnixStream) -> Result<Option<ServerMsg>, ()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    match protocol::read_frame(stream) {
        Ok(bytes) => protocol::decode_server(&bytes).map(Some).map_err(|_| ()),
        Err(err) if err.kind() == ErrorKind::TimedOut || err.kind() == ErrorKind::WouldBlock => {
            Ok(None)
        }
        Err(_) => Err(()),
    }
}

fn helper_identity(pid: u32, start: u64, session: &str, owner_pid: u32) -> bool {
    if pid < 2 || start == 0 || owner_pid < 2 {
        return false;
    }
    if procinfo::process_start_ticks(pid) != Some(start) || procinfo::process_uid(pid) != Some(0) {
        return false;
    }
    let argv = procinfo::process_argv(pid).unwrap_or_default();
    match procinfo::process_exe(pid) {
        Some(exe) => install::helper_process_matches(&exe, &argv, session, owner_pid),
        None => {
            let installed_ok = install::assert_root_owned_tree(Path::new(HELPER_PATH)).is_ok();
            install::helper_unreadable_exe_matches(&argv, session, owner_pid, installed_ok)
        }
    }
}

fn socket_peer_is_root_pid(stream: &UnixStream, hello_pid: u32) -> bool {
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut libc::ucred as *mut libc::c_void,
            &mut len,
        )
    };
    rc == 0 && install::socket_peer_accepts(cred.uid, cred.pid as u32, hello_pid)
}

fn ensure_installed() -> Result<(), String> {
    if !Path::new(PKEXEC_PATH).is_file() {
        return Err("polkit is not installed".into());
    }
    install::assert_root_owned_tree(Path::new(HELPER_PATH))
        .map_err(|_| "the installed VPN helper is missing".to_string())?;
    install::assert_root_owned_tree(Path::new(SIDECAR_PATH))
        .map_err(|_| "the installed core is missing".to_string())?;
    Ok(())
}

fn safe_failure(code: Option<i32>, stderr: &str, redactor: &Redactor) -> String {
    let cleaned = redactor.apply(stderr);
    for known in [
        "the desktop app must not run as root",
        "a managed core is already running",
        "the installed VPN helper is missing",
        "the installed core is missing",
        "the helper must run as root",
        "session id is invalid",
        "ipc peer is not the desktop process",
        "the desktop process is not the expected owner",
    ] {
        if cleaned.contains(known) {
            return known.to_string();
        }
    }
    install::classify_pkexec_failure(code, &cleaned)
}

fn stop_pkexec(child: &mut Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    let pid = child.id();
    if pid < 2 {
        return;
    }
    unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn wait_dead(pid: u32, start: u64, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !procinfo::process_alive(pid, start) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// pkexec execs the helper, so the GUI child is that process. After it exits,
/// `/proc/pid/stat` remains until wait, with the same starttime and state `Z`.
/// Reaping the child, or seeing `Z`/`X`, means the helper is already gone.
fn owned_helper_stopped(child: &mut Child, pid: u32, start: u64) -> bool {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return true;
    }
    !procinfo::process_alive(pid, start)
}

fn wait_owned_helper(child: &mut Child, pid: u32, start: u64, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if owned_helper_stopped(child, pid, start) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ActiveFile {
    helper_pid: u32,
    helper_start: u64,
    session: String,
}

fn write_active(data_dir: &Path, pid: u32, start: u64, session: &str) -> Result<(), String> {
    let active = ActiveFile {
        helper_pid: pid,
        helper_start: start,
        session: session.to_string(),
    };
    let bytes = serde_json::to_vec(&active)
        .map_err(|_| "could not store the active session".to_string())?;
    fsutil::write_private(&data_dir.join("active.json"), &bytes)
        .map_err(|_| "could not store the active session".to_string())
}

fn read_active(data_dir: &Path) -> Option<ActiveFile> {
    let bytes = fs::read(data_dir.join("active.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn clear_active(data_dir: &Path) {
    let _ = fs::remove_file(data_dir.join("active.json"));
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

fn profile_path(state: &AppState) -> Result<PathBuf, String> {
    Ok(ensure_data_dir(state)?.join("profile.json"))
}

fn ensure_data_dir(state: &AppState) -> Result<PathBuf, String> {
    let dir = data_dir()?;
    fsutil::ensure_dir(&dir)
        .map_err(|_| "could not create the application directory".to_string())?;
    lock_inner(&state.shared).data_dir = dir.clone();
    Ok(dir)
}

fn data_dir() -> Result<PathBuf, String> {
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        let path = PathBuf::from(&xdg);
        if path.is_absolute() {
            return Ok(path.join("mieru"));
        }
    }
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    let home = PathBuf::from(home);
    if !home.is_absolute() {
        return Err("HOME is not an absolute path".into());
    }
    Ok(home.join(".local/share/mieru"))
}

fn new_nonce() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    let mut file =
        File::open("/dev/urandom").map_err(|_| "could not generate a session id".to_string())?;
    file.read_exact(&mut bytes)
        .map_err(|_| "could not generate a session id".to_string())?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
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

fn lock_inner(shared: &Shared) -> std::sync::MutexGuard<'_, Inner> {
    shared.inner.lock().unwrap_or_else(|err| err.into_inner())
}

fn lock_op(shared: &Shared) -> std::sync::MutexGuard<'_, ()> {
    shared.op.lock().unwrap_or_else(|err| err.into_inner())
}
