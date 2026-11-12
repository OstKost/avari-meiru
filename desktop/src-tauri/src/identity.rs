use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug)]
pub struct ProcessFacts {
    pub exe: PathBuf,
    pub start_sec: u64,
    pub args: String,
}

#[derive(Clone, Debug)]
pub struct ProcessClaim {
    pub pid: u32,
    pub exe: PathBuf,
    pub start_sec: u64,
    pub marker: String,
    pub dir: Option<PathBuf>,
}

#[repr(C)]
struct ProcBsdInfo {
    pbi_flags: u32,
    pbi_status: u32,
    pbi_xstatus: u32,
    pbi_pid: u32,
    pbi_ppid: u32,
    pbi_uid: u32,
    pbi_gid: u32,
    pbi_ruid: u32,
    pbi_rgid: u32,
    pbi_svuid: u32,
    pbi_svgid: u32,
    rfu_1: u32,
    pbi_comm: [u8; 16],
    pbi_name: [u8; 32],
    pbi_nfiles: u32,
    pbi_pgid: u32,
    pbi_pjobc: u32,
    e_tdev: u32,
    e_tpgid: u32,
    pbi_nice: i32,
    pbi_start_tvsec: u64,
    pbi_start_tvusec: u64,
}

const PROC_PIDTBSDINFO: i32 = 3;

#[link(name = "proc")]
unsafe extern "C" {
    fn proc_pidpath(pid: i32, buffer: *mut libc::c_void, buffersize: u32) -> i32;
    fn proc_pidinfo(
        pid: i32,
        flavor: i32,
        arg: u64,
        buffer: *mut libc::c_void,
        buffersize: i32,
    ) -> i32;
}

pub fn inspect(pid: u32) -> Option<ProcessFacts> {
    if pid == 0 {
        return None;
    }
    let exe = pid_path(pid)?;
    let start_sec = start_sec(pid)?;
    let args = process_args(pid).unwrap_or_default();
    Some(ProcessFacts {
        exe,
        start_sec,
        args,
    })
}

pub fn claim_matches(claim: &ProcessClaim) -> bool {
    let Some(facts) = inspect(claim.pid) else {
        return false;
    };
    if !same_file(&facts.exe, &claim.exe)
        || facts.start_sec != claim.start_sec
        || claim.marker.is_empty()
    {
        return false;
    }
    if facts.args.contains(&claim.marker) {
        return true;
    }
    if let Some(dir) = &claim.dir {
        if crate::supervisor::confirm_handshake(dir, claim.pid, &claim.marker) {
            return true;
        }
    }
    false
}

pub fn same_file(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn pid_path(pid: u32) -> Option<PathBuf> {
    let mut buffer = vec![0u8; 4096];
    let length = unsafe {
        proc_pidpath(
            pid as i32,
            buffer.as_mut_ptr() as *mut libc::c_void,
            buffer.len() as u32,
        )
    };
    if length <= 0 {
        return None;
    }
    let length = length as usize;
    if length > buffer.len() {
        return None;
    }
    buffer.truncate(length);
    let text = String::from_utf8(buffer).ok()?;
    Some(PathBuf::from(text))
}

pub fn start_sec(pid: u32) -> Option<u64> {
    if let Some(sec) = start_sec_proc_pidinfo(pid) {
        return Some(sec);
    }
    if let Some(sec) = start_sec_sysctl(pid) {
        return Some(sec);
    }
    start_sec_ps(pid)
}

pub fn start_sec_proc_pidinfo(pid: u32) -> Option<u64> {
    let mut info = std::mem::MaybeUninit::<ProcBsdInfo>::zeroed();
    let size = std::mem::size_of::<ProcBsdInfo>() as i32;
    let written = unsafe {
        proc_pidinfo(
            pid as i32,
            PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr() as *mut libc::c_void,
            size,
        )
    };
    if written < size {
        return None;
    }
    let info = unsafe { info.assume_init() };
    if info.pbi_pid != pid || info.pbi_start_tvsec == 0 {
        return None;
    }
    Some(info.pbi_start_tvsec)
}

pub fn start_sec_sysctl(pid: u32) -> Option<u64> {
    if pid == 0 {
        return None;
    }
    let mut mib = [
        libc::CTL_KERN,
        libc::KERN_PROC,
        libc::KERN_PROC_PID,
        pid as i32,
    ];
    // macOS kinfo_proc: offset 0 is struct timeval p_starttime (tv_sec: c_long, tv_usec: c_int32)
    let mut buf = [0u8; 1024];
    let mut len = buf.len();
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            4,
            buf.as_mut_ptr() as *mut libc::c_void,
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || len < 8 {
        return None;
    }
    let tv_sec = i64::from_ne_bytes(buf[0..8].try_into().ok()?);
    if tv_sec <= 0 {
        return None;
    }
    Some(tv_sec as u64)
}

pub fn start_sec_ps(pid: u32) -> Option<u64> {
    if pid == 0 {
        return None;
    }
    let output = Command::new("/bin/ps")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    parse_lstart_utc(text.trim())
}

pub fn parse_lstart_utc(text: &str) -> Option<u64> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    if parts.len() != 5 {
        return None;
    }
    let normalized = parts.join(" ");
    let naive = chrono::NaiveDateTime::parse_from_str(&normalized, "%a %b %e %H:%M:%S %Y")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(&normalized, "%a %b %d %H:%M:%S %Y"))
        .ok()?;
    let sec = naive.and_utc().timestamp();
    if sec <= 0 {
        return None;
    }
    Some(sec as u64)
}

fn process_args(pid: u32) -> Option<String> {
    let output = Command::new("/bin/ps")
        .args(["-ww", "-p", &pid.to_string(), "-o", "command="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_process_layout_matches_kernel() {
        let pid = std::process::id();
        let facts = inspect(pid).expect("inspect self");
        assert_eq!(facts.start_sec > 0, true);
        let mut info = std::mem::MaybeUninit::<ProcBsdInfo>::zeroed();
        let size = std::mem::size_of::<ProcBsdInfo>() as i32;
        let written = unsafe {
            proc_pidinfo(
                pid as i32,
                PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr() as *mut libc::c_void,
                size,
            )
        };
        assert_eq!(written, size, "proc_bsdinfo size drifted");
        let info = unsafe { info.assume_init() };
        assert_eq!(info.pbi_pid, pid);
    }

    #[test]
    fn fallback_matches_kernel_for_own_process() {
        let pid = std::process::id();
        let kernel_sec = start_sec_proc_pidinfo(pid).expect("kernel proc_pidinfo for self");
        let sysctl_sec = start_sec_sysctl(pid).expect("sysctl for self");
        let ps_sec = start_sec_ps(pid).expect("ps fallback for self");

        assert_eq!(
            kernel_sec, sysctl_sec,
            "proc_pidinfo and sysctl start_sec must match"
        );
        assert_eq!(
            kernel_sec, ps_sec,
            "kernel and ps fallback start_sec must match without timezone drift"
        );
    }

    #[test]
    fn inspect_root_pid1_is_some_and_negative_claims_fail() {
        let facts = inspect(1).expect("inspect pid 1 (launchd) must succeed");
        assert!(facts.start_sec > 0);
        assert!(!facts.args.is_empty());
        assert!(same_file(&facts.exe, Path::new("/sbin/launchd")));

        // Negative claims against PID 1 must not match
        let wrong_exe = ProcessClaim {
            pid: 1,
            exe: PathBuf::from("/Applications/Mieru.app/Contents/MacOS/mieru-desktop"),
            start_sec: facts.start_sec,
            marker: "valid_looking_marker".to_string(),
            dir: None,
        };
        assert!(!claim_matches(&wrong_exe));

        let wrong_start = ProcessClaim {
            pid: 1,
            exe: PathBuf::from("/sbin/launchd"),
            start_sec: facts.start_sec + 100,
            marker: "launchd".to_string(),
            dir: None,
        };
        assert!(!claim_matches(&wrong_start));

        let wrong_marker = ProcessClaim {
            pid: 1,
            exe: PathBuf::from("/sbin/launchd"),
            start_sec: facts.start_sec,
            marker: "nonexistent_session_nonce_123456789".to_string(),
            dir: None,
        };
        assert!(!claim_matches(&wrong_marker));
    }

    #[test]
    fn parse_lstart_utc_formats() {
        assert_eq!(
            parse_lstart_utc("Wed Nov 11 06:36:55 2026"),
            Some(1794379015)
        );
        assert!(parse_lstart_utc("Sun Nov  1 06:36:55 2026").is_some());
    }
}
