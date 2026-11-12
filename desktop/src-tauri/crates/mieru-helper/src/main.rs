#![cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]

mod dns;
mod probe;
mod service;

use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    unsafe {
        libc::setsid();
    }
    let stop = Arc::new(AtomicBool::new(false));
    if signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stop)).is_err()
        || signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&stop)).is_err()
    {
        eprintln!("mieru-helper: could not watch for stop signals");
        return ExitCode::from(1);
    }
    match run(stop) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("mieru-helper: {err}");
            ExitCode::from(1)
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() -> ExitCode {
    eprintln!("mieru-helper runs on Linux");
    ExitCode::from(1)
}

#[cfg(target_os = "linux")]
fn run(stop: Arc<AtomicBool>) -> Result<(), String> {
    if unsafe { libc::geteuid() } != 0 {
        return Err("the helper must run as root".into());
    }
    let args = parse_args()?;
    let uid = mieru_shared::procinfo::process_uid(args.owner_pid)
        .ok_or_else(|| "the desktop process is not the expected owner".to_string())?;
    let gid = mieru_shared::procinfo::process_gid(args.owner_pid)
        .ok_or_else(|| "the desktop process is not the expected owner".to_string())?;
    let start = mieru_shared::procinfo::process_start_ticks(args.owner_pid)
        .ok_or_else(|| "the desktop process is not the expected owner".to_string())?;
    if uid == 0 {
        return Err("the desktop app must not run as root".into());
    }
    let user_dir = mieru_shared::install::runtime_session_dir(uid, &args.session)?;
    let socket_path = mieru_shared::install::session_socket(uid, &args.session)?;
    let root_dir = mieru_shared::install::root_session_dir(&args.session)?;
    prepare_root_runtime()?;
    let alive_stop = Arc::clone(&stop);
    let owner_pid = args.owner_pid;
    let config = service::Config {
        session: args.session,
        owner_pid,
        owner_uid: uid,
        owner_gid: gid,
        socket_path,
        user_dir,
        root_dir,
        lock_path: Path::new("/run/mieru/instance.lock").to_path_buf(),
        resolv_path: Some(Path::new("/etc/resolv.conf").to_path_buf()),
        enforce_install: true,
        enforce_peer: true,
        use_resolvectl: Path::new("/usr/bin/resolvectl").is_file(),
        limits: service::Limits::production(),
        stop: Arc::clone(&stop),
        owner_alive: Arc::new(move || {
            !alive_stop.load(Ordering::SeqCst)
                && mieru_shared::procinfo::process_alive(owner_pid, start)
        }),
    };
    service::serve(config, &service::FixedSidecar, &probe::probe_tunnel)
}

#[cfg(target_os = "linux")]
struct Args {
    session: String,
    owner_pid: u32,
}

#[cfg(target_os = "linux")]
fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let mut session = None;
    let mut owner_pid = None;
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| "unexpected helper arguments".to_string())?;
        match flag.as_str() {
            "--session" if session.is_none() => session = Some(value),
            "--owner-pid" if owner_pid.is_none() => owner_pid = Some(value),
            _ => return Err("unexpected helper arguments".into()),
        }
    }
    let session = session.ok_or_else(|| "missing session".to_string())?;
    let owner_text = owner_pid.ok_or_else(|| "missing owner".to_string())?;
    if !mieru_shared::install::is_session_id(&session) {
        return Err("session id is invalid".into());
    }
    let owner_pid: u32 = owner_text
        .parse()
        .map_err(|_| "owner pid is not numeric".to_string())?;
    if owner_pid < 2 {
        return Err("refusing to use a system pid".into());
    }
    Ok(Args { session, owner_pid })
}

#[cfg(target_os = "linux")]
fn prepare_root_runtime() -> Result<(), String> {
    std::fs::create_dir_all("/run/mieru").map_err(|_| "could not create the session directory")?;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    std::fs::set_permissions("/run/mieru", std::fs::Permissions::from_mode(0o755))
        .map_err(|_| "could not create the session directory")?;
    let meta = std::fs::symlink_metadata("/run/mieru")
        .map_err(|_| "could not create the session directory".to_string())?;
    if meta.file_type().is_symlink()
        || !meta.is_dir()
        || meta.uid() != 0
        || meta.mode() & 0o022 != 0
    {
        return Err("could not create the session directory".into());
    }
    Ok(())
}
