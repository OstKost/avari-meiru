use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::sync::{Mutex, MutexGuard, TryLockError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

use crate::profile::{Profile, Transport};
use crate::vpn::AppState;

const PROBE_BUDGET: Duration = Duration::from_secs(8);
const WATCHDOG_KILL_AFTER_SECS: u64 = 9;
const READY_MARK: &str = "sing-box started (";
const PROBE_HOST: &str = "example.com";
const PROBE_PORT: u16 = 80;

/// Opt-in network smoke check. Uses the backend's saved profile without
/// printing its address or credentials, and never starts a TUN session.
#[cfg(test)]
#[test]
#[ignore = "requires a saved working server profile and network access"]
fn saved_server_live_smoke() {
    let result = tauri::async_runtime::block_on(check(AppState::new()))
        .expect("health command should return a sanitized result");
    println!("{}", serde_json::to_string(&result).unwrap());
    assert_eq!(result.state, ServerHealthState::Ready);
}

static PROBE: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerHealthState {
    Ready,
    Error,
    Unconfigured,
    Busy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServerHealth {
    pub state: ServerHealthState,
    pub latency_ms: Option<u64>,
    pub message: String,
    pub checked_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeFail {
    Deadline,
    Refused,
    Malformed,
    Core,
    Setup,
    NoCore,
}

enum CoreEvent {
    Ready,
    Ended,
}

struct ProbeSession {
    dir: PathBuf,
    child: Option<Child>,
    stdin: Option<std::process::ChildStdin>,
    stderr_thread: Option<JoinHandle<()>>,
    stop: std::sync::Arc<AtomicBool>,
}

struct StartedProbe {
    events: Receiver<CoreEvent>,
    session: ProbeSession,
    port: u16,
}

pub async fn check(state: AppState) -> Result<ServerHealth, String> {
    match tauri::async_runtime::spawn_blocking(move || check_blocking(state)).await {
        Ok(result) => result,
        Err(_) => Err("Проверка сервера прервана".to_string()),
    }
}

fn check_blocking(state: AppState) -> Result<ServerHealth, String> {
    let Some(_flight) = try_begin_probe() else {
        return Ok(report(
            ServerHealthState::Busy,
            None,
            "Проверка уже выполняется",
        ));
    };
    let loaded = saved_profile(&state);
    Ok(interpret(loaded, |profile| {
        let binary = crate::core_config::sidecar_path().map_err(|_| ProbeFail::NoCore)?;
        run_probe(
            profile,
            &binary,
            &std::env::temp_dir(),
            Instant::now() + PROBE_BUDGET,
            None,
        )
    }))
}

fn try_begin_probe() -> Option<MutexGuard<'static, ()>> {
    match PROBE.try_lock() {
        Ok(guard) => Some(guard),
        Err(TryLockError::Poisoned(err)) => Some(err.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

#[cfg(target_os = "macos")]
fn saved_profile(state: &AppState) -> Result<Option<Profile>, String> {
    crate::vpn::load_saved_profile(state)
}

#[cfg(target_os = "linux")]
fn saved_profile(state: &AppState) -> Result<Option<Profile>, String> {
    let _ = state;
    let xdg = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let path = linux_profile_file(xdg.as_deref(), home.as_deref())?;
    crate::profile::load(&path)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn saved_profile(_state: &AppState) -> Result<Option<Profile>, String> {
    Err("could not create the application directory".into())
}

#[cfg(any(test, target_os = "linux"))]
fn linux_profile_file(xdg: Option<&Path>, home: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(xdg) = xdg {
        if xdg.is_absolute() {
            return Ok(xdg.join("mieru").join("profile.json"));
        }
    }
    let home = home.ok_or("HOME is not set")?;
    if !home.is_absolute() {
        return Err("HOME is not an absolute path".into());
    }
    Ok(home.join(".local/share/mieru/profile.json"))
}

fn interpret(
    loaded: Result<Option<Profile>, String>,
    probe: impl FnOnce(&Profile) -> Result<u64, ProbeFail>,
) -> ServerHealth {
    let profile = match loaded {
        Ok(Some(profile)) => profile,
        Ok(None) => return report(ServerHealthState::Unconfigured, None, "Сервер не настроен"),
        Err(err) => return report(ServerHealthState::Error, None, saved_profile_message(&err)),
    };
    match probe(&profile) {
        Ok(latency) => report(ServerHealthState::Ready, Some(latency), "Сервер отвечает"),
        Err(fail) => report(ServerHealthState::Error, None, fail.message()),
    }
}

fn saved_profile_message(err: &str) -> &'static str {
    if err.contains("invalid") || err.contains("must be") {
        "Сохранённый профиль некорректен"
    } else {
        "Не удалось подготовить проверку"
    }
}

fn report(state: ServerHealthState, latency_ms: Option<u64>, message: &str) -> ServerHealth {
    ServerHealth {
        state,
        latency_ms,
        message: message.to_string(),
        checked_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    }
}

impl ProbeFail {
    fn message(self) -> &'static str {
        match self {
            ProbeFail::Deadline => "Сервер не ответил вовремя",
            ProbeFail::Refused => "Сервер отклонил соединение",
            ProbeFail::Malformed => "Ответ сервера не распознан",
            ProbeFail::Core => "Ядро проверки остановилось",
            ProbeFail::Setup => "Не удалось подготовить проверку",
            ProbeFail::NoCore => "Ядро не найдено",
        }
    }
}

fn run_probe(
    profile: &Profile,
    binary: &Path,
    parent: &Path,
    deadline: Instant,
    pidfile: Option<&Path>,
) -> Result<u64, ProbeFail> {
    let started = start_session(profile, binary, parent, pidfile)?;
    let result = complete_probe(&started, deadline);
    // Session owns the child and the private directory; dropping it is the cleanup.
    let StartedProbe { session, .. } = started;
    drop(session);
    result
}

fn start_session(
    profile: &Profile,
    binary: &Path,
    parent: &Path,
    pidfile: Option<&Path>,
) -> Result<StartedProbe, ProbeFail> {
    let mut session = ProbeSession::create(parent)?;
    let port = prepare_probe(&session.dir, profile)?;
    let events = spawn_core(&mut session, binary, pidfile)?;
    Ok(StartedProbe {
        events,
        session,
        port,
    })
}

impl ProbeSession {
    fn create(parent: &Path) -> Result<Self, ProbeFail> {
        let dir = parent.join(probe_dir_name()?);
        crate::fsutil::ensure_dir(&dir).map_err(|_| ProbeFail::Setup)?;
        Ok(Self {
            dir,
            child: None,
            stdin: None,
            stderr_thread: None,
            stop: std::sync::Arc::new(AtomicBool::new(false)),
        })
    }
}

impl Drop for ProbeSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.stdin.take();
        let dir = self.dir.clone();
        if let Some(child) = self.child.as_mut() {
            terminate_group(child, &dir);
        }
        if let Some(handle) = self.stderr_thread.take() {
            let _ = handle.join();
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn probe_dir_name() -> Result<String, ProbeFail> {
    let mut bytes = [0u8; 8];
    let mut file = File::open("/dev/urandom").map_err(|_| ProbeFail::Setup)?;
    file.read_exact(&mut bytes).map_err(|_| ProbeFail::Setup)?;
    let mut name = format!("mieru-health-{}-", std::process::id());
    for byte in bytes {
        name.push_str(&format!("{byte:02x}"));
    }
    Ok(name)
}

fn prepare_probe(dir: &Path, profile: &Profile) -> Result<u16, ProbeFail> {
    let port = crate::core_config::pick_mixed_port().map_err(|_| ProbeFail::Setup)?;
    let body = build_probe_config(profile, port).map_err(|_| ProbeFail::Setup)?;
    let value: Value = serde_json::from_str(&body).map_err(|_| ProbeFail::Setup)?;
    ensure_probe_shape(&value).map_err(|_| ProbeFail::Setup)?;
    let workdir = dir.join("work");
    crate::fsutil::ensure_dir(&workdir).map_err(|_| ProbeFail::Setup)?;
    crate::fsutil::write_private(&dir.join("config.json"), body.as_bytes())
        .map_err(|_| ProbeFail::Setup)?;
    Ok(port)
}

fn build_probe_config(profile: &Profile, mixed_port: u16) -> Result<String, String> {
    let mut outbound = json!({
        "type": "mieru",
        "tag": "mieru-out",
        "server": profile.server,
        "server_port": profile.server_port,
        "transport": match profile.transport {
            Transport::TCP => "TCP",
            Transport::UDP => "UDP",
        },
        "username": profile.username,
        "password": profile.password,
    });
    if let Some(pattern) = &profile.traffic_pattern {
        outbound["traffic_pattern"] = Value::String(pattern.clone());
    }
    let config = json!({
        "log": { "level": "info" },
        "dns": {
            "servers": [{
                "type": "udp",
                "tag": "remote-dns",
                "server": "1.1.1.1",
                "server_port": 53,
                "detour": "mieru-out"
            }],
            "final": "remote-dns",
            "strategy": "ipv4_only"
        },
        "inbounds": [{
            "type": "mixed",
            "tag": "mixed-in",
            "listen": "127.0.0.1",
            "listen_port": mixed_port
        }],
        "outbounds": [outbound],
        "route": {
            "final": "mieru-out",
            "auto_detect_interface": true
        }
    });
    serde_json::to_string_pretty(&config)
        .map_err(|_| "could not encode the core configuration".into())
}

fn ensure_probe_shape(value: &Value) -> Result<(), ()> {
    let inbounds = value.get("inbounds").and_then(Value::as_array).ok_or(())?;
    if inbounds.len() != 1 {
        return Err(());
    }
    let inbound = &inbounds[0];
    if inbound.get("type").and_then(Value::as_str) != Some("mixed") {
        return Err(());
    }
    if inbound.get("listen").and_then(Value::as_str) != Some("127.0.0.1") {
        return Err(());
    }
    let port = inbound
        .get("listen_port")
        .and_then(Value::as_u64)
        .ok_or(())?;
    if port == 0 || port > 65535 {
        return Err(());
    }
    if value
        .get("route")
        .and_then(|route| route.get("final"))
        .and_then(Value::as_str)
        != Some("mieru-out")
    {
        return Err(());
    }
    if json_contains_forbidden(value) {
        return Err(());
    }
    Ok(())
}

fn json_contains_forbidden(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("tun") {
                return true;
            }
            if map.contains_key("auto_route")
                || map.contains_key("auto_redirect")
                || map.contains_key("interface_name")
            {
                return true;
            }
            map.values().any(json_contains_forbidden)
        }
        Value::Array(items) => items.iter().any(json_contains_forbidden),
        _ => false,
    }
}

struct WatchdogSpec {
    script: String,
    singbox: PathBuf,
    config: PathBuf,
    workdir: PathBuf,
    pidfile: Option<PathBuf>,
}

fn watchdog_script() -> String {
    // Braces that belong to the shell are doubled so `format!` leaves them alone.
    format!(
        r#"set -u
exec 3<&0
child=0
reader=0
cleaned=0
ppid_of() {{
  ps -o ppid= -p "$1" 2>/dev/null | tr -d '[:space:]'
}}
is_own_child() {{
  [ "$1" -gt 1 ] || return 1
  [ "$(ppid_of "$1")" = "$$" ]
}}
reap_owned() {{
  pid=$1
  [ "$pid" -gt 1 ] || return 0
  stat=$(ps -o stat= -p "$pid" 2>/dev/null | tr -d '[:space:]')
  case "$stat" in
    ""|Z*)
      wait "$pid" 2>/dev/null || true
      return 0
      ;;
  esac
  is_own_child "$pid" || return 1
  kill -TERM "$pid" 2>/dev/null || true
  sleep 1
  if is_own_child "$pid"; then
    kill -KILL "$pid" 2>/dev/null || true
  fi
  wait "$pid" 2>/dev/null || true
  stat=$(ps -o stat= -p "$pid" 2>/dev/null | tr -d '[:space:]')
  case "$stat" in
    ""|Z*) return 0 ;;
  esac
  return 1
}}
remove_own_dir() {{
  dir=${{PROBE_DIR:-}}
  case "$dir" in
    /*) ;;
    *) return 0 ;;
  esac
  case "$dir" in
    *..*) return 0 ;;
  esac
  base=${{dir##*/}}
  case "$base" in
    mieru-health-[0-9]*) ;;
    *) return 0 ;;
  esac
  case "$base" in
    *[!A-Za-z0-9-]*) return 0 ;;
  esac
  [ -d "$dir" ] || return 0
  [ -L "$dir" ] && return 0
  rm -rf -- "$dir"
}}
cleanup() {{
  [ "$cleaned" -eq 0 ] || return 0
  cleaned=1
  reap_owned "$reader" || true
  reader=0
  if reap_owned "$child"; then
    child=0
    remove_own_dir
  fi
}}
trap 'cleanup' EXIT INT TERM HUP
"$SINGBOX" run -c "$CONFIG" -D "$WORKDIR" --disable-color </dev/null 3<&- &
child=$!
echo "$child" > "$PROBE_DIR/core.pid" 2>/dev/null || true
# Explicit stdin redirect: bash otherwise points background jobs at /dev/null.
cat <&3 >/dev/null 3<&- &
reader=$!
exec 3<&-
i=0
while [ "$i" -lt {secs} ]; do
  stat=$(ps -o stat= -p "$reader" 2>/dev/null | tr -d '[:space:]')
  case "$stat" in
    ""|Z*) break ;;
  esac
  sleep 1
  i=$((i + 1))
done
cleanup
trap - EXIT
exit 0
"#,
        secs = WATCHDOG_KILL_AFTER_SECS,
    )
}

fn watchdog_spec(
    binary: &Path,
    dir: &Path,
    pidfile: Option<&Path>,
) -> Result<WatchdogSpec, ProbeFail> {
    let config = dir.join("config.json");
    let workdir = dir.join("work");
    for path in [binary, dir, config.as_path(), workdir.as_path()] {
        if path_has_control(path) {
            return Err(ProbeFail::Setup);
        }
    }
    if let Some(path) = pidfile {
        if path_has_control(path) {
            return Err(ProbeFail::Setup);
        }
    }
    Ok(WatchdogSpec {
        script: watchdog_script(),
        singbox: binary.to_path_buf(),
        config,
        workdir,
        pidfile: pidfile.map(Path::to_path_buf),
    })
}

fn path_has_control(path: &Path) -> bool {
    match path.to_str() {
        Some(text) => text.chars().any(|ch| ch.is_control()),
        None => true,
    }
}

fn spawn_core(
    session: &mut ProbeSession,
    binary: &Path,
    pidfile: Option<&Path>,
) -> Result<Receiver<CoreEvent>, ProbeFail> {
    let spec = watchdog_spec(binary, &session.dir, pidfile)?;
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c")
        .arg(&spec.script)
        .env("SINGBOX", &spec.singbox)
        .env("CONFIG", &spec.config)
        .env("WORKDIR", &spec.workdir)
        .env("PROBE_DIR", &session.dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(path) = &spec.pidfile {
        cmd.env("MIERU_HEALTH_PIDFILE", path);
    }
    // The shell becomes its own process-group leader so a later kill reaches
    // sing-box, and so the GUI's process group does not take the probe with it
    // before the stdin pipe can close.
    // SAFETY: setpgid is async-signal-safe and this closure does nothing else.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    let mut child = cmd.spawn().map_err(|_| ProbeFail::Setup)?;
    let stdin = child.stdin.take();
    let stderr = child.stderr.take().ok_or(ProbeFail::Setup)?;
    let (tx, rx) = mpsc::sync_channel(8);
    let stop = std::sync::Arc::clone(&session.stop);
    let stderr_thread = thread::spawn(move || drain_stderr(stderr, tx, stop));
    session.child = Some(child);
    session.stdin = stdin;
    session.stderr_thread = Some(stderr_thread);
    Ok(rx)
}

fn drain_stderr(
    mut stderr: std::process::ChildStderr,
    tx: SyncSender<CoreEvent>,
    stop: std::sync::Arc<AtomicBool>,
) {
    let _ = set_nonblocking(stderr.as_raw_fd());
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    let mut announced = false;
    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match stderr.read(&mut tmp) {
            Ok(0) => {
                let _ = tx.send(CoreEvent::Ended);
                break;
            }
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if !announced && contains_ready(&buf) {
                    announced = true;
                    if tx.send(CoreEvent::Ready).is_err() {
                        break;
                    }
                }
                if buf.len() > 64 * 1024 {
                    let keep = READY_MARK.len();
                    let start = buf.len() - keep;
                    buf.drain(..start);
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(15));
            }
            Err(_) => {
                let _ = tx.send(CoreEvent::Ended);
                break;
            }
        }
    }
}

fn contains_ready(buf: &[u8]) -> bool {
    buf.windows(READY_MARK.len())
        .any(|window| window == READY_MARK.as_bytes())
}

fn set_nonblocking(fd: i32) -> bool {
    // SAFETY: fcntl F_GETFL/F_SETFL on a descriptor this process owns.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 {
            return false;
        }
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) >= 0
    }
}

fn terminate_group(shell: &mut Child, dir: &Path) {
    let Ok(shell_pid) = i32::try_from(shell.id()) else {
        return;
    };
    if shell_pid <= 1 {
        return;
    }
    // Snapshot the core before any signal. Shell exit alone is not proof that a
    // core which ignored SIGTERM has exited, and the pid must not be signaled
    // again after it has been reused.
    let core_pid = read_core_pid(dir);
    let stamp = core_pid.and_then(process_stamp);
    signal_group(shell_pid, libc::SIGTERM);
    if !wait_shell(shell, Duration::from_millis(400)) {
        // The shell is still alive, so this process group is still the one we created.
        signal_group(shell_pid, libc::SIGKILL);
        let _ = shell.wait();
    }
    if let (Some(pid), Some(stamp)) = (core_pid, stamp) {
        escalate_same_process(pid, stamp);
    }
}

fn read_core_pid(dir: &Path) -> Option<u32> {
    let text = fs::read_to_string(dir.join("core.pid")).ok()?;
    let pid = text.trim().parse::<u32>().ok()?;
    if pid > 1 {
        Some(pid)
    } else {
        None
    }
}

fn process_stamp(pid: u32) -> Option<u64> {
    if pid <= 1 {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        return crate::identity::start_sec(pid);
    }
    #[cfg(target_os = "linux")]
    {
        return mieru_shared::procinfo::process_start_ticks(pid);
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

fn wait_shell(shell: &mut Child, limit: Duration) -> bool {
    let started = Instant::now();
    loop {
        match shell.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) if started.elapsed() >= limit => return false,
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(_) => return true,
        }
    }
}

fn escalate_same_process(pid: u32, stamp: u64) {
    if !same_stamp(pid, stamp) {
        return;
    }
    signal_one(pid, libc::SIGTERM);
    if wait_until_stamp_changes(pid, stamp, Duration::from_millis(400)) {
        return;
    }
    if same_stamp(pid, stamp) {
        signal_one(pid, libc::SIGKILL);
    }
    let _ = wait_until_stamp_changes(pid, stamp, Duration::from_millis(400));
}

fn same_stamp(pid: u32, stamp: u64) -> bool {
    process_stamp(pid) == Some(stamp)
}

fn wait_until_stamp_changes(pid: u32, stamp: u64, limit: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < limit {
        if !same_stamp(pid, stamp) {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    !same_stamp(pid, stamp)
}

fn signal_one(pid: u32, sig: i32) {
    let Ok(pid) = i32::try_from(pid) else {
        return;
    };
    if pid <= 1 {
        return;
    }
    // SAFETY: pid was checked against the start stamp immediately before this call.
    unsafe {
        libc::kill(pid, sig);
    }
}

fn signal_group(pid: i32, sig: i32) {
    // SAFETY: pid is the probe shell, not this process; a negative pid targets its group.
    unsafe {
        libc::kill(-pid, sig);
    }
}

fn complete_probe(started: &StartedProbe, deadline: Instant) -> Result<u64, ProbeFail> {
    wait_ready(&started.events, deadline)?;
    let addr = SocketAddr::from(([127, 0, 0, 1], started.port));
    request_via_socks(addr, deadline, || !core_stopped(&started.events))
}

fn wait_ready(events: &Receiver<CoreEvent>, deadline: Instant) -> Result<(), ProbeFail> {
    let timeout = remaining(deadline)?;
    match events.recv_timeout(timeout) {
        Ok(CoreEvent::Ready) => Ok(()),
        Ok(CoreEvent::Ended) => Err(ProbeFail::Core),
        Err(RecvTimeoutError::Timeout) => Err(ProbeFail::Deadline),
        Err(RecvTimeoutError::Disconnected) => Err(ProbeFail::Core),
    }
}

fn core_stopped(events: &Receiver<CoreEvent>) -> bool {
    match events.try_recv() {
        Ok(CoreEvent::Ended) | Err(TryRecvError::Disconnected) => true,
        Ok(CoreEvent::Ready) | Err(TryRecvError::Empty) => false,
    }
}

fn remaining(deadline: Instant) -> Result<Duration, ProbeFail> {
    let now = Instant::now();
    if now >= deadline {
        Err(ProbeFail::Deadline)
    } else {
        Ok(deadline.saturating_duration_since(now))
    }
}

fn request_via_socks(
    addr: SocketAddr,
    deadline: Instant,
    mut alive: impl FnMut() -> bool,
) -> Result<u64, ProbeFail> {
    if !addr.ip().is_loopback() {
        return Err(ProbeFail::Setup);
    }
    loop {
        if !alive() {
            return Err(ProbeFail::Core);
        }
        let timeout = remaining(deadline)?;
        let started = Instant::now();
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(stream) => return http_roundtrip(stream, started, deadline),
            Err(err) if retryable_connect(&err) => {
                if Instant::now() >= deadline {
                    return Err(ProbeFail::Deadline);
                }
                thread::sleep(Duration::from_millis(20).min(remaining(deadline)?));
            }
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {
                return Err(ProbeFail::Deadline);
            }
            Err(_) => return Err(ProbeFail::Core),
        }
    }
}

fn retryable_connect(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::AddrNotAvailable
    )
}

fn http_roundtrip(
    mut stream: TcpStream,
    started: Instant,
    deadline: Instant,
) -> Result<u64, ProbeFail> {
    write_all_deadline(&mut stream, &[0x05, 0x01, 0x00], deadline)?;
    let mut greeting = [0u8; 2];
    read_exact_deadline(&mut stream, &mut greeting, deadline)?;
    if greeting[0] != 0x05 || greeting[1] != 0x00 {
        return Err(ProbeFail::Refused);
    }
    let mut request = Vec::with_capacity(22);
    request.extend_from_slice(&[0x05, 0x01, 0x00, 0x03, PROBE_HOST.len() as u8]);
    request.extend_from_slice(PROBE_HOST.as_bytes());
    request.extend_from_slice(&PROBE_PORT.to_be_bytes());
    write_all_deadline(&mut stream, &request, deadline)?;
    read_socks_reply(&mut stream, deadline)?;
    let http =
        format!("GET / HTTP/1.1\r\nHost: {PROBE_HOST}\r\nConnection: close\r\nAccept: */*\r\n\r\n");
    write_all_deadline(&mut stream, http.as_bytes(), deadline)?;
    let line = read_status_line(&mut stream, deadline)?;
    parse_http_status(&line)?;
    Ok(started.elapsed().as_millis() as u64)
}

fn read_socks_reply(stream: &mut TcpStream, deadline: Instant) -> Result<(), ProbeFail> {
    let mut header = [0u8; 4];
    read_exact_deadline(stream, &mut header, deadline)?;
    if header[0] != 0x05 {
        return Err(ProbeFail::Malformed);
    }
    let outcome = if header[1] == 0x00 {
        Ok(())
    } else {
        Err(ProbeFail::Refused)
    };
    let extra = match header[3] {
        0x01 => 6,
        0x04 => 18,
        0x03 => {
            let mut len = [0u8; 1];
            read_exact_deadline(stream, &mut len, deadline)?;
            len[0] as usize + 2
        }
        _ => return Err(ProbeFail::Malformed),
    };
    let mut rest = vec![0u8; extra];
    read_exact_deadline(stream, &mut rest, deadline)?;
    outcome
}

fn read_status_line(stream: &mut TcpStream, deadline: Instant) -> Result<Vec<u8>, ProbeFail> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 64];
    while buf.len() < 256 {
        let timeout = remaining(deadline)?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|_| ProbeFail::Setup)?;
        match stream.read(&mut tmp) {
            Ok(0) => return Err(ProbeFail::Malformed),
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if let Some(pos) = buf.windows(2).position(|window| window == b"\r\n") {
                    return Ok(buf[..pos].to_vec());
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err)
                if err.kind() == std::io::ErrorKind::TimedOut
                    || err.kind() == std::io::ErrorKind::WouldBlock =>
            {
                return Err(ProbeFail::Deadline);
            }
            Err(_) => return Err(ProbeFail::Malformed),
        }
    }
    Err(ProbeFail::Malformed)
}

fn parse_http_status(line: &[u8]) -> Result<u16, ProbeFail> {
    let rest = line.strip_prefix(b"HTTP/1.").ok_or(ProbeFail::Malformed)?;
    if rest.first().copied() != Some(b'0') && rest.first().copied() != Some(b'1') {
        return Err(ProbeFail::Malformed);
    }
    let rest = &rest[1..];
    if rest.first().copied() != Some(b' ') {
        return Err(ProbeFail::Malformed);
    }
    let rest = &rest[1..];
    if rest.len() < 3 || !rest[..3].iter().all(u8::is_ascii_digit) {
        return Err(ProbeFail::Malformed);
    }
    if rest.len() > 3 && rest[3] != b' ' {
        return Err(ProbeFail::Malformed);
    }
    let code = (u16::from(rest[0] - b'0') * 100)
        + (u16::from(rest[1] - b'0') * 10)
        + u16::from(rest[2] - b'0');
    if !(100..600).contains(&code) {
        return Err(ProbeFail::Malformed);
    }
    Ok(code)
}

fn read_exact_deadline(
    stream: &mut TcpStream,
    buf: &mut [u8],
    deadline: Instant,
) -> Result<(), ProbeFail> {
    let mut offset = 0;
    while offset < buf.len() {
        let timeout = remaining(deadline)?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|_| ProbeFail::Setup)?;
        match stream.read(&mut buf[offset..]) {
            Ok(0) => return Err(ProbeFail::Malformed),
            Ok(n) => offset += n,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err)
                if err.kind() == std::io::ErrorKind::TimedOut
                    || err.kind() == std::io::ErrorKind::WouldBlock =>
            {
                return Err(ProbeFail::Deadline);
            }
            Err(_) => return Err(ProbeFail::Malformed),
        }
    }
    Ok(())
}

fn write_all_deadline(
    stream: &mut TcpStream,
    bytes: &[u8],
    deadline: Instant,
) -> Result<(), ProbeFail> {
    let timeout = remaining(deadline)?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|_| ProbeFail::Setup)?;
    stream.write_all(bytes).map_err(|err| {
        if err.kind() == std::io::ErrorKind::TimedOut
            || err.kind() == std::io::ErrorKind::WouldBlock
        {
            ProbeFail::Deadline
        } else {
            ProbeFail::Malformed
        }
    })
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::net::TcpListener;
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc;

    use super::*;

    fn sample() -> Profile {
        Profile {
            server: "health.example.test".into(),
            server_port: 8964,
            transport: Transport::TCP,
            username: "example-user".into(),
            password: "example-password".into(),
            traffic_pattern: Some("GgQIARAK".into()),
        }
    }

    #[test]
    fn config_is_loopback_without_tun_and_keeps_hostname() {
        let text = build_probe_config(&sample(), 12111).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert!(ensure_probe_shape(&value).is_ok());
        assert_eq!(value["inbounds"][0]["listen"], "127.0.0.1");
        assert_eq!(value["inbounds"][0]["listen_port"], 12111);
        assert_eq!(value["inbounds"][0]["type"], "mixed");
        assert_eq!(value["inbounds"].as_array().unwrap().len(), 1);
        assert_eq!(value["outbounds"][0]["server"], "health.example.test");
        assert_eq!(value["outbounds"][0]["type"], "mieru");
        assert_eq!(value["outbounds"][0]["transport"], "TCP");
        assert_eq!(value["dns"]["servers"][0]["detour"], "mieru-out");
        assert_eq!(value["route"]["auto_detect_interface"], true);
        assert!(value.get("auto_route").is_none());
        assert!(!text.contains("\"tun\""));
        assert!(!text.contains("auto_route"));
        assert!(!text.contains("auto_redirect"));
        let tun = json!({"inbounds": [{"type": "tun", "auto_route": true}]});
        assert!(ensure_probe_shape(&tun).is_err());
    }

    #[test]
    fn written_config_is_private() {
        let parent = std::env::temp_dir().join(format!("mieru-health-mode-{}", std::process::id()));
        let _ = fs::remove_dir_all(&parent);
        crate::fsutil::ensure_dir(&parent).unwrap();
        let dir = parent.join("probe");
        crate::fsutil::ensure_dir(&dir).unwrap();
        let port = prepare_probe(&dir, &sample()).unwrap();
        assert!(port >= 12080 && port < 12180);
        let path = dir.join("config.json");
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dir_mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("example-password"));
        assert!(text.contains("127.0.0.1"));
        assert!(!text.contains("\"tun\""));
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn watchdog_keeps_secret_out_of_script_and_env_names() {
        let spec = watchdog_spec(
            Path::new("/tmp/sing-box"),
            Path::new("/tmp/mieru-health-spec"),
            Some(Path::new("/tmp/probe.pid")),
        )
        .unwrap();
        assert!(spec.script.contains("-lt 9"));
        assert!(spec.script.contains("sleep 1"));
        assert!(spec.script.contains("kill -KILL"));
        assert!(spec.script.contains("rm -rf --"));
        assert!(spec.script.contains("mieru-health-"));
        assert!(spec.script.contains("is_own_child"));
        assert!(spec.script.contains("exec 3<&0"));
        assert!(spec.script.contains("</dev/null 3<&- &"));
        assert!(spec.script.contains("cat <&3 >/dev/null 3<&- &"));
        assert!(spec.script.contains("exec 3<&-"));
        assert_eq!(spec.script.matches("rm -rf").count(), 1);
        assert!(!spec.script.contains("sleep 9"));
        assert!(!spec.script.contains("sleep 1 &"));
        assert!(!spec.script.contains("example-password"));
        assert!(!spec.script.contains("GgQIARAK"));
        assert_eq!(spec.script.matches("SINGBOX").count(), 1);
        let joined = format!(
            "{} {} {} {}",
            spec.singbox.display(),
            spec.config.display(),
            spec.workdir.display(),
            spec.pidfile.unwrap().display()
        );
        assert!(!joined.contains("example-password"));
        let syntax = Command::new("/bin/sh")
            .args(["-n", "-c", &spec.script])
            .output()
            .expect("sh -n");
        assert!(
            syntax.status.success(),
            "watchdog script rejected: {}",
            String::from_utf8_lossy(&syntax.stderr)
        );
    }

    #[test]
    fn http_status_parser_accepts_real_responses_only() {
        assert_eq!(parse_http_status(b"HTTP/1.1 200 OK").unwrap(), 200);
        assert_eq!(parse_http_status(b"HTTP/1.0 301 Moved").unwrap(), 301);
        assert_eq!(parse_http_status(b"HTTP/1.1 404 Not Found").unwrap(), 404);
        assert!(parse_http_status(b"HTTP/1.1 099").is_err());
        assert!(parse_http_status(b"HTTP/2 200").is_err());
        assert!(parse_http_status(b"not http").is_err());
        assert!(parse_http_status(b"HTTP/1.1 20").is_err());
    }

    #[test]
    fn states_serialize_as_strings() {
        let health = interpret(Ok(None), |_| panic!("probe must not run"));
        assert_eq!(health.state, ServerHealthState::Unconfigured);
        assert!(health.latency_ms.is_none());
        let json = serde_json::to_value(&health).unwrap();
        assert_eq!(json["state"], "unconfigured");
        assert!(json["latency_ms"].is_null());
        assert!(chrono::DateTime::parse_from_rfc3339(json["checked_at"].as_str().unwrap()).is_ok());

        let ready = interpret(Ok(Some(sample())), |_| Ok(42));
        assert_eq!(ready.state, ServerHealthState::Ready);
        assert_eq!(ready.latency_ms, Some(42));
        assert_eq!(ready.message, "Сервер отвечает");
        assert!(!ready.message.contains("example-password"));

        let invalid = interpret(Err("saved profile is invalid".into()), |_| unreachable!());
        assert_eq!(invalid.message, "Сохранённый профиль некорректен");
        let setup = interpret(Err("HOME is not set".into()), |_| unreachable!());
        assert_eq!(setup.message, "Не удалось подготовить проверку");
        let late = interpret(Ok(Some(sample())), |_| Err(ProbeFail::Deadline));
        assert_eq!(late.state, ServerHealthState::Error);
        assert_eq!(late.message, "Сервер не ответил вовремя");
        assert!(late.latency_ms.is_none());
        assert!(!late.message.contains("example-password"));
    }

    #[test]
    fn linux_profile_path_matches_the_vpn_rule() {
        let xdg = Path::new("/tmp/xdg");
        let home = Path::new("/Users/example");
        assert_eq!(
            linux_profile_file(Some(xdg), Some(home)).unwrap(),
            PathBuf::from("/tmp/xdg/mieru/profile.json")
        );
        assert_eq!(
            linux_profile_file(Some(Path::new("relative")), Some(home)).unwrap(),
            PathBuf::from("/Users/example/.local/share/mieru/profile.json")
        );
        assert!(linux_profile_file(None, Some(Path::new("relative"))).is_err());
        assert!(linux_profile_file(None, None).is_err());
    }

    #[test]
    fn socks_success_reads_http_status_without_local_dns() {
        let (addr, report) = spawn_socks(SocksScript::Ok);
        let latency =
            request_via_socks(addr, Instant::now() + Duration::from_secs(2), || true).unwrap();
        assert!(latency < 2_000);
        assert_eq!(report.recv_timeout(Duration::from_secs(2)).unwrap(), "ok");
    }

    #[test]
    fn socks_refusal_is_not_success() {
        let (addr, _report) = spawn_socks(SocksScript::Refuse);
        let err =
            request_via_socks(addr, Instant::now() + Duration::from_secs(2), || true).unwrap_err();
        assert_eq!(err, ProbeFail::Refused);
        assert_eq!(err.message(), "Сервер отклонил соединение");
    }

    #[test]
    fn malformed_http_is_rejected() {
        let (addr, _report) = spawn_socks(SocksScript::Garbage);
        let err =
            request_via_socks(addr, Instant::now() + Duration::from_secs(2), || true).unwrap_err();
        assert_eq!(err, ProbeFail::Malformed);
    }

    #[test]
    fn socks_deadline_stays_bounded() {
        let (addr, _report) = spawn_socks(SocksScript::Silent);
        let started = Instant::now();
        let err =
            request_via_socks(addr, started + Duration::from_millis(250), || true).unwrap_err();
        assert_eq!(err, ProbeFail::Deadline);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn non_loopback_proxy_is_refused_before_connect() {
        let addr = "203.0.113.10:80".parse().unwrap();
        let started = Instant::now();
        let err = request_via_socks(addr, started + Duration::from_secs(2), || true).unwrap_err();
        assert_eq!(err, ProbeFail::Setup);
        assert!(started.elapsed() < Duration::from_millis(200));
    }

    enum SocksScript {
        Ok,
        Refuse,
        Garbage,
        Silent,
    }

    fn spawn_socks(script: SocksScript) -> (SocketAddr, mpsc::Receiver<&'static str>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let Ok((mut sock, _)) = listener.accept() else {
                let _ = tx.send("accept");
                return;
            };
            let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
            let _ = sock.set_write_timeout(Some(Duration::from_secs(2)));
            if matches!(script, SocksScript::Silent) {
                thread::sleep(Duration::from_secs(2));
                let _ = tx.send("silent");
                return;
            }
            let mut greeting = [0u8; 3];
            if sock.read_exact(&mut greeting).is_err() || greeting != [5, 1, 0] {
                let _ = tx.send("greeting");
                return;
            }
            if sock.write_all(&[5, 0]).is_err() {
                let _ = tx.send("write");
                return;
            }
            let mut head = [0u8; 4];
            if sock.read_exact(&mut head).is_err() {
                let _ = tx.send("head");
                return;
            }
            if head != [5, 1, 0, 3] {
                let _ = tx.send("not-domain");
                return;
            }
            let mut len = [0u8; 1];
            if sock.read_exact(&mut len).is_err() {
                let _ = tx.send("len");
                return;
            }
            let mut host = vec![0u8; len[0] as usize];
            if sock.read_exact(&mut host).is_err() || host != b"example.com" {
                let _ = tx.send("host");
                return;
            }
            let mut port = [0u8; 2];
            if sock.read_exact(&mut port).is_err() || port != 80u16.to_be_bytes() {
                let _ = tx.send("port");
                return;
            }
            if matches!(script, SocksScript::Refuse) {
                let _ = sock.write_all(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]);
                let _ = tx.send("ok");
                return;
            }
            if sock.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).is_err() {
                let _ = tx.send("reply");
                return;
            }
            if matches!(script, SocksScript::Garbage) {
                let _ = sock.write_all(b"not-http\r\n");
                let _ = tx.send("ok");
                return;
            }
            let mut req = Vec::new();
            let mut tmp = [0u8; 128];
            while !req.windows(4).any(|window| window == b"\r\n\r\n") && req.len() < 1024 {
                match sock.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => req.extend_from_slice(&tmp[..n]),
                }
            }
            if !req.starts_with(b"GET / HTTP/1.1\r\n")
                || !req.windows(17).any(|w| w == b"Host: example.com")
            {
                let _ = tx.send("http");
                return;
            }
            let _ = sock.write_all(
                b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            let _ = tx.send("ok");
        });
        (addr, rx)
    }

    #[test]
    fn live_probe_is_stopped_and_files_removed() {
        let parent = std::env::temp_dir().join(format!(
            "mieru-health-live-{}-{}",
            std::process::id(),
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&parent);
        crate::fsutil::ensure_dir(&parent).unwrap();
        let binary = write_fake_core(&parent);
        let pidfile = parent.join("core.pid");
        let started = start_session(&sample(), &binary, &parent, Some(&pidfile)).unwrap();
        let dir = started.session.dir.clone();
        assert!(dir.starts_with(&parent));
        assert!(dir.join("config.json").is_file());
        let pid = wait_pid(&pidfile);
        drop(started);
        assert!(!dir.exists(), "probe directory leaked");
        assert!(!process_alive(pid), "probe process {pid} survived");
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn stdin_loss_without_drop_removes_core_and_config() {
        if std::env::var_os("MIERU_HEALTH_HARNESS").is_some() {
            harness_forget_session_and_exit();
        }
        let parent = std::env::temp_dir().join(format!("mieru-health-eof-{}", std::process::id()));
        let _ = fs::remove_dir_all(&parent);
        crate::fsutil::ensure_dir(&parent).unwrap();
        fs::write(parent.join("keep"), b"sentinel").unwrap();
        let decoy = parent.join("mieru-health-9-decoy");
        crate::fsutil::ensure_dir(&decoy).unwrap();
        fs::write(decoy.join("keep"), b"decoy").unwrap();
        let mut leftover = ProbeLeftover {
            parent: parent.clone(),
            pid: None,
            stamp: None,
        };

        let exe = std::env::current_exe().unwrap();
        let mut harness = Command::new(exe)
            .args([
                "health::tests::stdin_loss_without_drop_removes_core_and_config",
                "--exact",
                "--test-threads=1",
            ])
            .env("MIERU_HEALTH_HARNESS", "1")
            .env("MIERU_HEALTH_PARENT", &parent)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let status = wait_harness(&mut harness, Duration::from_secs(10));
        let Some(status) = status else {
            let _ = harness.kill();
            let _ = harness.wait();
            panic!("harness did not exit");
        };
        let output = harness_output(&mut harness);
        assert!(
            status.success(),
            "harness exited {status}; {}",
            redact_harness(&output)
        );

        let probe_dir = PathBuf::from(fs::read_to_string(parent.join("probe.path")).unwrap());
        let pid = fs::read_to_string(parent.join("core.pid"))
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        let stamp = fs::read_to_string(parent.join("core.stamp"))
            .unwrap()
            .trim()
            .parse::<u64>()
            .unwrap();
        leftover.pid = Some(pid);
        leftover.stamp = Some(stamp);
        assert!(
            probe_dir.starts_with(&parent) && probe_dir != parent && probe_dir != decoy,
            "harness recorded an unexpected probe directory"
        );

        let started = Instant::now();
        let mut dir_gone = false;
        let mut process_gone = false;
        while started.elapsed() < Duration::from_secs(8) {
            dir_gone = !probe_dir.exists();
            process_gone = process_stamp(pid) != Some(stamp);
            if dir_gone && process_gone {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        assert!(
            dir_gone && process_gone,
            "cleanup incomplete: dir_gone={dir_gone} process_gone={process_gone} pid={pid}"
        );
        assert!(parent.join("keep").is_file(), "parent sentinel removed");
        assert!(decoy.join("keep").is_file(), "neighbor directory removed");
        assert_eq!(fs::read(decoy.join("keep")).unwrap(), b"decoy");
        leftover.pid = None;
    }

    #[test]
    fn failed_probe_cleans_up_within_deadline() {
        let parent = std::env::temp_dir().join(format!("mieru-health-fail-{}", std::process::id()));
        let _ = fs::remove_dir_all(&parent);
        crate::fsutil::ensure_dir(&parent).unwrap();
        let binary = write_fake_core(&parent);
        let pidfile = parent.join("fail.pid");
        let clock = Instant::now();
        let err = run_probe(
            &sample(),
            &binary,
            &parent,
            clock + Duration::from_millis(700),
            Some(&pidfile),
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                ProbeFail::Deadline | ProbeFail::Core | ProbeFail::Refused
            ),
            "unexpected {err:?}"
        );
        assert!(clock.elapsed() < Duration::from_secs(4));
        assert_eq!(err.message().contains("example-password"), false);
        if pidfile.is_file() {
            let pid = fs::read_to_string(&pidfile)
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            assert!(!process_alive(pid));
        }
        assert!(
            !parent.read_dir().unwrap().any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("mieru-health-")),
            "probe directory leaked"
        );
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn sidecar_check_accepts_probe_config() {
        let sidecar = crate::core_config::sidecar_path().expect("sidecar");
        let parent =
            std::env::temp_dir().join(format!("mieru-health-check-{}", std::process::id()));
        let _ = fs::remove_dir_all(&parent);
        crate::fsutil::ensure_dir(&parent).unwrap();
        let dir = parent.join("probe");
        crate::fsutil::ensure_dir(&dir).unwrap();
        prepare_probe(&dir, &sample()).unwrap();
        let config = dir.join("config.json");
        let output = Command::new(&sidecar)
            .args(["check", "-c", config.to_str().unwrap(), "--disable-color"])
            .output()
            .expect("run check");
        let stderr =
            String::from_utf8_lossy(&output.stderr).replace("example-password", "[redacted]");
        assert!(
            !stderr.contains("example-password") && !stderr.contains("GgQIARAK"),
            "{stderr}"
        );
        assert!(output.status.success(), "{stderr}");
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn overlapping_check_is_busy_and_does_not_touch_profile() {
        let _flight = try_begin_probe().expect("probe lock");
        let health = check_blocking(AppState::new()).unwrap();
        assert_eq!(health.state, ServerHealthState::Busy);
        assert!(health.latency_ms.is_none());
        assert_eq!(health.message, "Проверка уже выполняется");
        assert!(!health.message.contains("example-password"));
        let again = tauri::async_runtime::block_on(check(AppState::new())).unwrap();
        assert_eq!(again.state, ServerHealthState::Busy);
    }

    fn harness_forget_session_and_exit() -> ! {
        let parent = PathBuf::from(std::env::var("MIERU_HEALTH_PARENT").unwrap());
        let binary = write_ignore_term_core(&parent);
        let pidfile = parent.join("core.pid");
        let started = start_session(&sample(), &binary, &parent, Some(&pidfile)).unwrap();
        let dir = started.session.dir.clone();
        let pid = wait_pid(&pidfile);
        assert!(
            dir.join("config.json").is_file(),
            "probe config was not written"
        );
        assert!(pid_alive_now(pid), "fake core was not alive");
        let stamp = process_stamp(pid).expect("start stamp");
        fs::write(parent.join("probe.path"), dir.to_str().unwrap()).unwrap();
        fs::write(parent.join("core.stamp"), stamp.to_string()).unwrap();
        // Exit skips ProbeSession::drop. The kernel closes the stdin pipe, which is the GUI SIGKILL path.
        std::mem::forget(started);
        std::process::exit(0);
    }

    fn write_ignore_term_core(parent: &Path) -> PathBuf {
        let path = parent.join("fake-core-ignore-term.sh");
        fs::write(
            &path,
            "#!/bin/sh\n\
             trap '' TERM\n\
             echo \"sing-box started (test\" >&2\n\
             if [ -n \"${MIERU_HEALTH_PIDFILE:-}\" ]; then\n\
               echo $$ > \"$MIERU_HEALTH_PIDFILE\"\n\
               fifo=\"${MIERU_HEALTH_PIDFILE}.fifo\"\n\
               rm -f \"$fifo\"\n\
               mkfifo \"$fifo\" || exit 1\n\
               exec 3<>\"$fifo\"\n\
               read -r _ <&3\n\
             fi\n\
             exit 0\n",
        )
        .unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o700);
        fs::set_permissions(&path, perms).unwrap();
        path
    }

    fn wait_harness(child: &mut Child, limit: Duration) -> Option<std::process::ExitStatus> {
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) if started.elapsed() >= limit => return None,
                Ok(None) => thread::sleep(Duration::from_millis(30)),
                Err(_) => return None,
            }
        }
    }

    fn harness_output(child: &mut Child) -> String {
        let mut out = String::new();
        if let Some(mut stdout) = child.stdout.take() {
            let _ = stdout.read_to_string(&mut out);
        }
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut out);
        }
        out
    }

    fn redact_harness(text: &str) -> String {
        let redacted = text
            .replace("example-password", "[redacted]")
            .replace("GgQIARAK", "[redacted]");
        let mut short: String = redacted.chars().take(400).collect();
        if redacted.chars().count() > 400 {
            short.push('…');
        }
        short
    }

    struct ProbeLeftover {
        parent: PathBuf,
        pid: Option<u32>,
        stamp: Option<u64>,
    }

    impl Drop for ProbeLeftover {
        fn drop(&mut self) {
            if let (Some(pid), Some(stamp)) = (self.pid, self.stamp) {
                if process_stamp(pid) == Some(stamp) {
                    signal_one(pid, libc::SIGKILL);
                }
            }
            let _ = fs::remove_dir_all(&self.parent);
        }
    }

    fn pid_alive_now(pid: u32) -> bool {
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        unsafe { libc::kill(pid, 0) == 0 }
    }

    fn write_fake_core(parent: &Path) -> PathBuf {
        let path = parent.join("fake-core.sh");
        fs::write(
            &path,
            "#!/bin/sh\n\
             echo \"sing-box started (test\" >&2\n\
             if [ -n \"${MIERU_HEALTH_PIDFILE:-}\" ]; then\n\
               echo $$ > \"$MIERU_HEALTH_PIDFILE\"\n\
             fi\n\
             sleep 30\n",
        )
        .unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o700);
        fs::set_permissions(&path, perms).unwrap();
        path
    }

    fn wait_pid(path: &Path) -> u32 {
        let start = Instant::now();
        loop {
            if let Ok(text) = fs::read_to_string(path) {
                if let Ok(pid) = text.trim().parse::<u32>() {
                    return pid;
                }
            }
            if start.elapsed() > Duration::from_secs(2) {
                panic!("fake core did not write a pid");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn process_alive(pid: u32) -> bool {
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(1) {
            let alive = unsafe { libc::kill(pid, 0) == 0 };
            if !alive {
                return false;
            }
            thread::sleep(Duration::from_millis(20));
        }
        unsafe { libc::kill(pid, 0) == 0 }
    }
}
