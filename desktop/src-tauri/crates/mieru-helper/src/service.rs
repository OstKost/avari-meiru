use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use mieru_shared::core_config::{self, check_core_config};
use mieru_shared::fsutil;
use mieru_shared::install::{self, SIDECAR_PATH};
use mieru_shared::profile::Profile;
use mieru_shared::protocol::{self, ClientMsg, ServerMsg};
use mieru_shared::redact::Redactor;

use crate::dns::{self, ResolvBackup};

const READY_MARK: &str = "sing-box started (";

pub struct Limits {
    pub ready: Duration,
    pub heartbeat: Duration,
    pub term_wait: Duration,
    pub accept: Duration,
}

impl Limits {
    pub fn production() -> Self {
        Self {
            ready: Duration::from_secs(15),
            heartbeat: Duration::from_secs(90),
            term_wait: Duration::from_secs(8),
            accept: Duration::from_secs(180),
        }
    }
}

pub struct Config {
    pub session: String,
    pub owner_pid: u32,
    pub owner_uid: u32,
    pub owner_gid: u32,
    pub socket_path: PathBuf,
    pub user_dir: PathBuf,
    pub root_dir: PathBuf,
    pub lock_path: PathBuf,
    pub resolv_path: Option<PathBuf>,
    pub enforce_install: bool,
    pub enforce_peer: bool,
    pub use_resolvectl: bool,
    pub limits: Limits,
    pub stop: Arc<AtomicBool>,
    pub owner_alive: Arc<dyn Fn() -> bool + Send + Sync>,
}

pub trait Core: Send + Sync {
    fn check(&self, config: &Path, redactor: &Redactor) -> Result<(), String>;
    fn spawn(&self, config: &Path, workdir: &Path, log: File) -> Result<Child, String>;
    fn confirm(&self, pid: u32, config: &Path) -> bool;
}

pub struct FixedSidecar;

impl Core for FixedSidecar {
    fn check(&self, config: &Path, redactor: &Redactor) -> Result<(), String> {
        check_core_config(Path::new(SIDECAR_PATH), config, redactor)
    }

    fn spawn(&self, config: &Path, workdir: &Path, log: File) -> Result<Child, String> {
        let log_err = log
            .try_clone()
            .map_err(|_| "core log is missing".to_string())?;
        let config_s = utf8_path(config)?;
        let workdir_s = utf8_path(workdir)?;
        Command::new(SIDECAR_PATH)
            .args(["run", "-c", &config_s, "-D", &workdir_s, "--disable-color"])
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .current_dir(workdir)
            .spawn()
            .map_err(|_| "could not start the core process".to_string())
    }

    fn confirm(&self, pid: u32, config: &Path) -> bool {
        confirm_fixed_sidecar(pid, config)
    }
}

pub fn serve(
    config: Config,
    core: &dyn Core,
    probe: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    if !install::is_session_id(&config.session) {
        return Err("session id is invalid".into());
    }
    if config.owner_uid == 0 || config.owner_pid < 2 {
        return Err("the desktop app must not run as root".into());
    }
    if !(config.owner_alive)() {
        return Err("the desktop process is not the expected owner".into());
    }
    if config.enforce_install {
        let expected = install::session_socket(config.owner_uid, &config.session)?;
        if config.socket_path != expected {
            return Err("session socket path is invalid".into());
        }
        let helper =
            std::env::current_exe().map_err(|_| "could not resolve the helper".to_string())?;
        if helper != Path::new(install::HELPER_PATH) {
            return Err("the installed VPN helper is missing".into());
        }
        install::assert_root_owned_tree(Path::new(install::HELPER_PATH))?;
        install::assert_root_owned_tree(Path::new(SIDECAR_PATH))?;
    }
    assert_user_dir(&config.user_dir, config.owner_uid)?;
    let _lock = Lock::acquire(&config.lock_path)?;
    fsutil::ensure_dir(&config.root_dir).map_err(|_| "could not create the session directory")?;
    let config_path = config.root_dir.join("config.json");
    let workdir = config.root_dir.join("workdir");
    fsutil::ensure_dir(&workdir).map_err(|_| "could not create the core work directory")?;
    let log_path = config.user_dir.join("core.log");
    let log_file = create_log(&log_path, config.owner_uid, config.owner_gid)?;

    let listener = bind_socket(&config.socket_path, config.owner_uid, config.owner_gid)?;
    let mut stream = accept_one(&listener, &config)?;
    if config.enforce_peer {
        check_peer(&stream, config.owner_uid, config.owner_pid)?;
    }
    let hello = read_client(&mut stream)?;
    match hello {
        ClientMsg::Hello { session } if session == config.session => {}
        _ => return Err("ipc session id does not match".into()),
    }
    protocol::write_frame(
        &mut stream,
        &ServerMsg::Hello {
            pid: std::process::id(),
            start: own_start(),
        },
    )
    .map_err(|_| "could not talk to the desktop".to_string())?;

    let connect = read_client(&mut stream)?;
    let profile = match connect {
        ClientMsg::Connect { profile } => profile,
        ClientMsg::Disconnect => return Ok(()),
        _ => return Err("ipc message is invalid".into()),
    };
    profile
        .validate()
        .map_err(|_| "profile is invalid".to_string())?;
    let redactor = Redactor::from_secrets(&profile.password, profile.traffic_pattern.as_deref());

    let mut session = Live {
        child: None,
        core,
        config_path: config_path.clone(),
        workdir,
        dns: None,
        tun: None,
        use_resolvectl: config.use_resolvectl,
        term_wait: config.limits.term_wait,
    };
    let result = run_core(
        &config,
        &mut session,
        &mut stream,
        &profile,
        &redactor,
        log_file,
        probe,
    );
    session.stop();
    result
}

struct Live<'a> {
    child: Option<Child>,
    core: &'a dyn Core,
    config_path: PathBuf,
    workdir: PathBuf,
    dns: Option<ResolvBackup>,
    tun: Option<String>,
    use_resolvectl: bool,
    term_wait: Duration,
}

impl Drop for Live<'_> {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Live<'_> {
    fn stop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            stop_child(child, self.core, &self.config_path, self.term_wait);
        }
        self.child = None;
        if let Some(tun) = self.tun.take() {
            if self.use_resolvectl {
                dns::resolvectl_revert(&tun);
            }
        }
        if let Some(dns) = self.dns.as_mut() {
            let _ = dns.restore();
        }
        let _ = fs::remove_file(&self.config_path);
        let _ = fs::remove_dir_all(&self.workdir);
    }
}

fn run_core(
    config: &Config,
    session: &mut Live<'_>,
    stream: &mut UnixStream,
    profile: &Profile,
    redactor: &Redactor,
    log_file: File,
    probe: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    let server_ip =
        core_config::resolve_server(&profile.server).map_err(|err| redact(redactor, &err))?;
    let mixed_port = core_config::pick_mixed_port().map_err(|err| redact(redactor, &err))?;
    let body = core_config::build_linux_core_config(profile, server_ip, mixed_port)
        .map_err(|err| redact(redactor, &err))?;
    fsutil::write_private(&session.config_path, body.as_bytes())
        .map_err(|_| "could not store the core configuration".to_string())?;
    if let Err(err) = session.core.check(&session.config_path, redactor) {
        return send_error(stream, &redact(redactor, &err));
    }
    session.child = Some(
        session
            .core
            .spawn(&session.config_path, &session.workdir, log_file)
            .map_err(|err| redact(redactor, &err))?,
    );
    let log_path = config.user_dir.join("core.log");
    if let Err(err) = wait_ready(config, session, stream, &log_path) {
        if err == "disconnect" {
            let _ = send_status(stream, "disconnected", "");
            return Ok(());
        }
        return send_error(stream, &redact(redactor, &err));
    }
    let _ = fs::remove_file(&session.config_path);
    if let Some(resolv) = &config.resolv_path {
        match ResolvBackup::redirect(resolv) {
            Ok(backup) => session.dns = Some(backup),
            Err(err) => return send_error(stream, &err),
        }
    }
    if config.use_resolvectl {
        if let Some(name) = install::tun_name_from_log(&read_log(&log_path)) {
            if dns::resolvectl_dns(&name).is_ok() {
                session.tun = Some(name);
            }
        }
    }
    if let Err(err) = probe() {
        return send_error(stream, &redact(redactor, &err));
    }
    send_status(stream, "connected", "")?;
    connected_loop(config, session, stream)
}

fn wait_ready(
    config: &Config,
    session: &mut Live<'_>,
    stream: &mut UnixStream,
    log_path: &Path,
) -> Result<(), String> {
    let deadline = Instant::now() + config.limits.ready;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    loop {
        if config.stop.load(Ordering::SeqCst) || !(config.owner_alive)() {
            return Err("the desktop process is not the expected owner".into());
        }
        if child_exited(&mut session.child) {
            return Err("core exited before it was ready".into());
        }
        if read_log(log_path).contains(READY_MARK) {
            return Ok(());
        }
        match read_client_timeout(stream) {
            Ok(Some(ClientMsg::Disconnect)) => return Err("disconnect".into()),
            Ok(Some(ClientMsg::Heartbeat { .. })) => {}
            Ok(Some(_)) => return Err("ipc message is invalid".into()),
            Ok(None) => {}
            Err(err) => return Err(err),
        }
        if Instant::now() >= deadline {
            return Err("core did not become ready within 15 seconds".into());
        }
    }
}

fn connected_loop(
    config: &Config,
    session: &mut Live<'_>,
    stream: &mut UnixStream,
) -> Result<(), String> {
    let mut last_beat = Instant::now();
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    loop {
        if config.stop.load(Ordering::SeqCst) {
            let _ = send_status(stream, "disconnected", "");
            return Ok(());
        }
        if !(config.owner_alive)() || last_beat.elapsed() > config.limits.heartbeat {
            return Err("the desktop process is not the expected owner".into());
        }
        if child_exited(&mut session.child) {
            return send_error(stream, "core exited unexpectedly");
        }
        match read_client_timeout(stream) {
            Ok(Some(ClientMsg::Heartbeat { .. })) => last_beat = Instant::now(),
            Ok(Some(ClientMsg::Disconnect)) => {
                let _ = send_status(stream, "disconnected", "");
                return Ok(());
            }
            Ok(Some(_)) => return send_error(stream, "ipc message is invalid"),
            Ok(None) => {}
            Err(_) => return Err("the desktop process is not the expected owner".into()),
        }
    }
}

fn child_exited(child: &mut Option<Child>) -> bool {
    let Some(child) = child.as_mut() else {
        return true;
    };
    match child.try_wait() {
        Ok(Some(_)) | Err(_) => true,
        Ok(None) => false,
    }
}

fn stop_child(child: &mut Child, core: &dyn Core, config: &Path, wait: Duration) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    let pid = child.id();
    if !core.confirm(pid, config) {
        return;
    }
    unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    let deadline = Instant::now() + wait;
    loop {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    if core.confirm(child.id(), config) {
        unsafe { libc::kill(child.id() as i32, libc::SIGKILL) };
    }
    let _ = child.wait();
}

fn send_error(stream: &mut UnixStream, message: &str) -> Result<(), String> {
    let _ = send_status(stream, "error", message);
    Err(message.to_string())
}

fn send_status(stream: &mut UnixStream, state: &str, message: &str) -> Result<(), String> {
    protocol::write_frame(
        stream,
        &ServerMsg::Status {
            state: state.to_string(),
            message: message.to_string(),
        },
    )
    .map_err(|_| "could not talk to the desktop".to_string())
}

fn read_client(stream: &mut UnixStream) -> Result<ClientMsg, String> {
    let _ = stream.set_read_timeout(None);
    let bytes =
        protocol::read_frame(&mut *stream).map_err(|_| "ipc message is invalid".to_string())?;
    protocol::decode_client(&bytes)
}

fn read_client_timeout(stream: &mut UnixStream) -> Result<Option<ClientMsg>, String> {
    if !poll_in(stream.as_raw_fd(), 200) {
        return Ok(None);
    }
    match protocol::read_frame(&mut *stream) {
        Ok(bytes) => protocol::decode_client(&bytes).map(Some),
        Err(err) if err.kind() == ErrorKind::UnexpectedEof => {
            Err("the desktop process is not the expected owner".into())
        }
        Err(_) => Err("ipc message is invalid".into()),
    }
}

fn poll_in(fd: i32, timeout_ms: i32) -> bool {
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    unsafe { libc::poll(&mut pollfd, 1, timeout_ms) > 0 }
}

fn bind_socket(path: &Path, uid: u32, gid: u32) -> Result<UnixListener, String> {
    let _ = fs::remove_file(path);
    let listener =
        UnixListener::bind(path).map_err(|_| "could not create the session socket".to_string())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|_| "could not create the session socket".to_string())?;
    chown(path, uid, gid)?;
    Ok(listener)
}

fn accept_one(listener: &UnixListener, config: &Config) -> Result<UnixStream, String> {
    listener
        .set_nonblocking(true)
        .map_err(|_| "could not create the session socket".to_string())?;
    let deadline = Instant::now() + config.limits.accept;
    loop {
        if config.stop.load(Ordering::SeqCst) || !(config.owner_alive)() {
            return Err("the desktop process is not the expected owner".into());
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                return Ok(stream);
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err("the desktop did not connect".into());
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return Err("could not create the session socket".into()),
        }
    }
}

#[cfg(target_os = "linux")]
fn check_peer(stream: &UnixStream, uid: u32, pid: u32) -> Result<(), String> {
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
    if rc != 0 || cred.uid != uid || cred.pid as u32 != pid {
        return Err("ipc peer is not the desktop process".into());
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn check_peer(_stream: &UnixStream, _uid: u32, _pid: u32) -> Result<(), String> {
    Err("ipc peer is not the desktop process".into())
}

fn confirm_fixed_sidecar(pid: u32, config: &Path) -> bool {
    #[cfg(target_os = "linux")]
    {
        let Some(exe) = mieru_shared::procinfo::process_exe(pid) else {
            return false;
        };
        let Some(argv) = mieru_shared::procinfo::process_argv(pid) else {
            return false;
        };
        let Some(marker) = config.to_str().filter(|text| !text.is_empty()) else {
            return false;
        };
        sidecar_identity_ok(&exe, &argv, marker)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (pid, config);
        false
    }
}

/// The pid is one this helper spawned. Native Linux reports the sidecar in
/// `/proc/pid/exe`. Docker Desktop's Rosetta (and qemu-user) report the
/// interpreter there. The installed path and this session's config must each
/// be a whole argv element, not a substring.
fn sidecar_identity_ok(exe: &Path, argv: &[String], config_path: &str) -> bool {
    install::argv_has_exact_token(argv, config_path)
        && install::installed_exe_argv_matches(exe, argv, SIDECAR_PATH)
}

fn own_start() -> u64 {
    #[cfg(target_os = "linux")]
    {
        mieru_shared::procinfo::process_start_ticks(std::process::id()).unwrap_or(1)
    }
    #[cfg(not(target_os = "linux"))]
    {
        1
    }
}

fn assert_user_dir(dir: &Path, uid: u32) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::symlink_metadata(dir).map_err(|_| "session directory is missing".to_string())?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err("session directory is missing".into());
    }
    if meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err("session directory is too open".into());
    }
    Ok(())
}

fn create_log(path: &Path, uid: u32, gid: u32) -> Result<File, String> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|_| "could not create the core log".to_string())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|_| "could not create the core log".to_string())?;
    chown(path, uid, gid)?;
    Ok(file)
}

fn chown(path: &Path, uid: u32, gid: u32) -> Result<(), String> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "a path cannot be used".to_string())?;
    let rc = unsafe { libc::chown(c.as_ptr(), uid, gid) };
    if rc == 0 {
        Ok(())
    } else {
        Err("could not create the session socket".into())
    }
}

fn read_log(path: &Path) -> String {
    fsutil::read_tail(path, 256 * 1024).unwrap_or_default()
}

fn redact(redactor: &Redactor, text: &str) -> String {
    let cleaned = redactor.apply(text);
    let mut out = String::new();
    for ch in cleaned.chars().take(300) {
        out.push(ch);
    }
    out
}

fn utf8_path(path: &Path) -> Result<String, String> {
    path.to_str()
        .filter(|text| !text.chars().any(|ch| ch.is_control()))
        .map(str::to_string)
        .ok_or_else(|| "a path cannot be used".to_string())
}

struct Lock {
    file: File,
}

impl Lock {
    fn acquire(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            fsutil::ensure_dir(parent)
                .map_err(|_| "could not lock the managed core".to_string())?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .open(path)
            .map_err(|_| "could not lock the managed core".to_string())?;
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            return Err("a managed core is already running".into());
        }
        Ok(Self { file })
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mieru_shared::profile::Transport;
    use std::sync::Mutex;

    struct FakeCore {
        spawned: Mutex<Option<u32>>,
    }

    impl Core for FakeCore {
        fn check(&self, config: &Path, redactor: &Redactor) -> Result<(), String> {
            let text =
                fs::read_to_string(config).map_err(|_| "configuration check failed".to_string())?;
            let cleaned = redactor.apply(&text);
            assert!(!cleaned.contains("example-password"));
            assert!(
                text.contains("\"dns_mode\": \"hijack\"")
                    || text.contains("\"dns_mode\": \"hijack\"")
            );
            assert!(text.contains("hijack-dns"));
            assert!(!text.contains("auto_redirect"));
            if text.contains("example-password") {
                // The file holds the secret; the checker must not echo it.
            }
            Ok(())
        }

        fn spawn(&self, config: &Path, _workdir: &Path, log: File) -> Result<Child, String> {
            assert!(config.ends_with("config.json"));
            use std::io::Write;
            let mut log = log;
            writeln!(log, "INFO sing-box started (0.01s)").map_err(|_| "core log is missing")?;
            writeln!(log, "inbound/tun[tun-in]: started at tun0")
                .map_err(|_| "core log is missing")?;
            let child = Command::new("/bin/sleep")
                .arg("30")
                .stdin(Stdio::null())
                .spawn()
                .map_err(|_| "could not start the core process".to_string())?;
            *self.spawned.lock().unwrap() = Some(child.id());
            Ok(child)
        }

        fn confirm(&self, pid: u32, _config: &Path) -> bool {
            self.spawned.lock().unwrap().as_ref() == Some(&pid)
                && unsafe { libc::kill(pid as i32, 0) } == 0
        }
    }

    #[test]
    fn sidecar_identity_accepts_native_exe_and_emulators_only_with_both_paths() {
        let config = "/run/mieru/0123456789abcdef0123456789abcdef/config.json";
        let native = vec![
            SIDECAR_PATH.to_string(),
            "run".into(),
            "-c".into(),
            config.to_string(),
            "-D".into(),
            "/run/mieru/work".into(),
            "--disable-color".into(),
        ];
        assert!(sidecar_identity_ok(Path::new(SIDECAR_PATH), &native, config));
        assert!(sidecar_identity_ok(
            Path::new("/run/rosetta/rosetta"),
            &native,
            config
        ));
        assert!(sidecar_identity_ok(
            Path::new("/usr/bin/qemu-x86_64-static"),
            &native,
            config
        ));
        assert!(!sidecar_identity_ok(
            Path::new("/run/rosetta/rosetta"),
            &["run".into(), "-c".into(), "/tmp/other.json".into()],
            config
        ));
        assert!(!sidecar_identity_ok(Path::new("/bin/sleep"), &native, config));
        assert!(!sidecar_identity_ok(
            Path::new(SIDECAR_PATH),
            &native,
            "/tmp/other.json"
        ));
        let embedded = vec![format!("{SIDECAR_PATH}-evil"), "-c".into(), config.into()];
        assert!(!sidecar_identity_ok(
            Path::new("/run/rosetta/rosetta"),
            &embedded,
            config
        ));
        let glued = vec![
            SIDECAR_PATH.to_string(),
            "-c".into(),
            format!("{config}.bak"),
        ];
        assert!(!sidecar_identity_ok(
            Path::new("/usr/bin/qemu-x86_64"),
            &glued,
            config
        ));
    }

    #[test]
    fn session_connects_checks_and_stops_without_leftover_process() {
        let uid = unsafe { libc::getuid() };
        let gid = unsafe { libc::getgid() };
        let base = PathBuf::from(format!("/tmp/mieru-h-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let user_dir = base.join("user");
        let root_dir = base.join("root");
        fsutil::ensure_dir(&user_dir).unwrap();
        fsutil::ensure_dir(&root_dir).unwrap();
        let resolv = base.join("resolv.conf");
        fs::write(&resolv, b"nameserver 127.0.0.53\n").unwrap();
        let socket_path = user_dir.join("helper.sock");
        let stop = Arc::new(AtomicBool::new(false));
        let config = Config {
            session: "0123456789abcdef0123456789abcdef".into(),
            owner_pid: std::process::id(),
            owner_uid: uid,
            owner_gid: gid,
            socket_path: socket_path.clone(),
            user_dir: user_dir.clone(),
            root_dir: root_dir.clone(),
            lock_path: base.join("instance.lock"),
            resolv_path: Some(resolv.clone()),
            enforce_install: false,
            enforce_peer: false,
            use_resolvectl: false,
            limits: Limits {
                ready: Duration::from_secs(3),
                heartbeat: Duration::from_secs(5),
                term_wait: Duration::from_millis(500),
                accept: Duration::from_secs(3),
            },
            stop: Arc::clone(&stop),
            owner_alive: Arc::new(|| true),
        };
        let core = FakeCore {
            spawned: Mutex::new(None),
        };
        let probe_ok = Arc::new(AtomicBool::new(true));
        let probe_flag = Arc::clone(&probe_ok);
        let server = thread::spawn(move || {
            serve(config, &core, &move || {
                if probe_flag.load(Ordering::SeqCst) {
                    Ok(())
                } else {
                    Err("tunnel check failed".into())
                }
            })
        });
        let mut client = wait_connect(&socket_path);
        protocol::write_frame(
            &mut client,
            &ClientMsg::Hello {
                session: "0123456789abcdef0123456789abcdef".into(),
            },
        )
        .unwrap();
        let hello = read_server(&mut client);
        match hello {
            ServerMsg::Hello { pid, start } => {
                assert!(pid >= 2);
                assert!(start >= 1);
            }
            other => panic!("unexpected {other:?}"),
        }
        protocol::write_frame(
            &mut client,
            &ClientMsg::Connect {
                profile: Profile {
                    server: "203.0.113.10".into(),
                    server_port: 8964,
                    transport: Transport::TCP,
                    username: "example-user".into(),
                    password: "example-password".into(),
                    traffic_pattern: None,
                },
            },
        )
        .unwrap();
        let status = read_server(&mut client);
        match status {
            ServerMsg::Status { state, message } => {
                assert_eq!(state, "connected");
                assert!(message.is_empty());
            }
            other => panic!("unexpected {other:?}"),
        }
        let during = fs::read_to_string(&resolv).unwrap();
        assert!(during.contains("172.19.0.2"));
        protocol::write_frame(&mut client, &ClientMsg::Disconnect).unwrap();
        let end = read_server(&mut client);
        match end {
            ServerMsg::Status { state, .. } => assert_eq!(state, "disconnected"),
            other => panic!("unexpected {other:?}"),
        }
        server.join().unwrap().unwrap();
        assert_eq!(fs::read(&resolv).unwrap(), b"nameserver 127.0.0.53\n");
        assert!(!root_dir.join("config.json").exists());
        assert!(!root_dir.join("workdir").exists());
        let _ = fs::remove_dir_all(&base);
    }

    fn wait_connect(path: &Path) -> UnixStream {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Ok(stream) = UnixStream::connect(path) {
                return stream;
            }
            if Instant::now() >= deadline {
                panic!("socket was not created");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn read_server(stream: &mut UnixStream) -> ServerMsg {
        let bytes = protocol::read_frame(stream).unwrap();
        protocol::decode_server(&bytes).unwrap()
    }
}
