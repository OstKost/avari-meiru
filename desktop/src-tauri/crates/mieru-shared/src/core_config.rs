use std::net::{IpAddr, TcpListener, ToSocketAddrs};
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

use crate::profile::{Profile, Transport};
use crate::redact::Redactor;

pub fn prefer_ipv4<I>(addrs: I) -> Option<IpAddr>
where
    I: IntoIterator<Item = IpAddr>,
{
    let mut fallback = None;
    for addr in addrs {
        if addr.is_ipv4() {
            return Some(addr);
        }
        if fallback.is_none() {
            fallback = Some(addr);
        }
    }
    fallback
}

pub fn resolve_server(server: &str) -> Result<IpAddr, String> {
    if let Ok(ip) = server.parse::<IpAddr>() {
        return Ok(ip);
    }
    let addrs = (server, 0u16)
        .to_socket_addrs()
        .map_err(|_| format!("could not resolve server {server}"))?;
    prefer_ipv4(addrs.map(|addr| addr.ip()))
        .ok_or_else(|| format!("could not resolve server {server}"))
}

pub fn pick_mixed_port() -> Result<u16, String> {
    for port in 12080u16..12180 {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Ok(port);
        }
    }
    Err("no free loopback port for the local proxy".into())
}

pub fn build_core_config(
    profile: &Profile,
    server_ip: IpAddr,
    mixed_port: u16,
) -> Result<String, String> {
    let exclude = match server_ip {
        IpAddr::V4(ip) => format!("{ip}/32"),
        IpAddr::V6(ip) => format!("{ip}/128"),
    };
    let mut outbound = json!({
        "type": "mieru",
        "tag": "mieru-out",
        "server": server_ip.to_string(),
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
        "inbounds": [
            {
                "type": "tun",
                "tag": "tun-in",
                "address": ["172.19.0.1/30", "fdfe:dcba:9876::1/126"],
                "auto_route": true,
                "stack": "gvisor",
                "route_exclude_address": [exclude]
            },
            {
                "type": "mixed",
                "tag": "mixed-in",
                "listen": "127.0.0.1",
                "listen_port": mixed_port
            }
        ],
        "outbounds": [outbound],
        "route": {
            "final": "mieru-out",
            "auto_detect_interface": true
        }
    });
    serde_json::to_string_pretty(&config)
        .map_err(|_| "could not encode the core configuration".to_string())
}

pub fn sidecar_file_name() -> Result<&'static str, String> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Ok("sing-box-aarch64-apple-darwin")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Ok("sing-box-x86_64-apple-darwin")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Ok("sing-box-x86_64-unknown-linux-gnu")
    } else {
        Err("this CPU architecture has no sidecar".into())
    }
}

/// Linux TUN config: the macOS document plus the fields exercised in
/// `core/LINUX_SMOKE.md` (`dns_mode: hijack` and the dns hijack route rule).
/// `auto_redirect` stays unset. IPv6 addresses are routed into the tun by
/// `auto_route`; that egress was not live-tested.
pub fn build_linux_core_config(
    profile: &Profile,
    server_ip: IpAddr,
    mixed_port: u16,
) -> Result<String, String> {
    let text = build_core_config(profile, server_ip, mixed_port)?;
    let mut value: Value =
        serde_json::from_str(&text).map_err(|_| "could not encode the core configuration")?;
    value["inbounds"][0]["dns_mode"] = json!("hijack");
    value["route"]["rules"] = json!([{
        "protocol": "dns",
        "action": "hijack-dns"
    }]);
    serde_json::to_string_pretty(&value)
        .map_err(|_| "could not encode the core configuration".to_string())
}

pub fn sidecar_path() -> Result<std::path::PathBuf, String> {
    #[cfg(target_os = "linux")]
    {
        let installed = Path::new(crate::install::SIDECAR_PATH);
        if installed.is_file() {
            crate::install::assert_root_owned_tree(installed)?;
            if accept_sidecar(installed, "sing-box").is_some() {
                return Ok(installed.to_path_buf());
            }
            return Err("installed core sidecar is not executable".into());
        }
    }
    let name = sidecar_file_name()?;
    let mut starts = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            starts.push(parent.to_path_buf());
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    starts.push(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    for start in starts {
        // Direct checks alongside exe or directory
        for direct in [
            start.join(name),
            start.join("sing-box"),
            start.join("Resources").join(name),
            start.join("Resources").join("sing-box"),
        ] {
            if let Some(path) = accept_sidecar(&direct, name) {
                return Ok(path);
            }
        }
        for ancestor in start.ancestors().take(8) {
            for candidate in [
                ancestor.join("core/dist").join(name),
                ancestor.join("core/dist").join("sing-box"),
                ancestor.join("MacOS").join(name),
                ancestor.join("MacOS").join("sing-box"),
                ancestor.join("Resources").join(name),
                ancestor.join("Resources").join("sing-box"),
            ] {
                if let Some(path) = accept_sidecar(&candidate, name) {
                    return Ok(path);
                }
            }
        }
    }
    Err(format!(
        "sidecar {name} was not found; build it with core/build-desktop.sh"
    ))
}

pub fn is_allowed_sidecar_name(name: &str) -> bool {
    if name == "sing-box" {
        return true;
    }
    if let Ok(arch_name) = sidecar_file_name() {
        if name == arch_name {
            return true;
        }
    }
    false
}

fn accept_sidecar(path: &Path, _name: &str) -> Option<std::path::PathBuf> {
    let canon = path.canonicalize().ok()?;
    if !canon.is_file() {
        return None;
    }
    let file_name = canon.file_name().and_then(|n| n.to_str())?;
    if !is_allowed_sidecar_name(file_name) {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = canon.metadata().ok()?.permissions().mode();
        if mode & 0o111 == 0 {
            return None;
        }
    }
    Some(canon)
}

pub fn check_core_config(sidecar: &Path, config: &Path, redactor: &Redactor) -> Result<(), String> {
    let output = Command::new(sidecar)
        .args([
            "check",
            "-c",
            config
                .to_str()
                .ok_or_else(|| "configuration path is not utf-8".to_string())?,
            "--disable-color",
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|_| "could not run the configuration check".to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = redactor.last_line(&stderr, 300);
    if detail.is_empty() {
        Err("configuration check failed".into())
    } else {
        Err(format!("configuration check failed: {detail}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsutil;
    use crate::profile::Profile;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    fn sample() -> Profile {
        Profile {
            server: "example.test".into(),
            server_port: 8964,
            transport: Transport::TCP,
            username: "example-user".into(),
            password: "example-password".into(),
            traffic_pattern: Some("GgQIARAK".into()),
        }
    }

    #[test]
    fn prefers_ipv4() {
        let picked = prefer_ipv4([
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            IpAddr::V4(Ipv4Addr::new(203, 0, 113, 10)),
        ]);
        assert_eq!(picked, Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 10))));
    }

    #[test]
    fn config_excludes_resolved_server_and_uses_gvisor() {
        let ip: IpAddr = "203.0.113.10".parse().unwrap();
        let text = build_core_config(&sample(), ip, 12080).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["log"]["level"], "info");
        assert!(value["log"].get("timestamp").is_none());
        assert_eq!(value["inbounds"][0]["stack"], "gvisor");
        assert_eq!(
            value["inbounds"][0]["route_exclude_address"][0],
            "203.0.113.10/32"
        );
        assert!(value["inbounds"][0].get("auto_redirect").is_none());
        assert!(value["inbounds"][0].get("interface_name").is_none());
        assert!(value["inbounds"][0].get("mtu").is_none());
        assert!(value["inbounds"][0].get("strict_route").is_none());
        assert_eq!(value["inbounds"][1]["listen"], "127.0.0.1");
        assert_eq!(value["inbounds"][1]["listen_port"], 12080);
        assert_eq!(value["outbounds"][0]["server"], "203.0.113.10");
        assert!(value["outbounds"][0].get("bind_interface").is_none());
        assert_eq!(value["dns"]["servers"][0]["detour"], "mieru-out");
        assert_eq!(value["dns"]["strategy"], "ipv4_only");
        assert_eq!(value["route"]["final"], "mieru-out");
        assert_eq!(value["route"]["auto_detect_interface"], true);
        assert!(value["route"].get("rules").is_none());
        assert!(value["inbounds"][0].get("dns_mode").is_none());
        assert!(!text.contains("example.test"));
    }

    #[test]
    fn linux_config_matches_tested_dns_hijack() {
        let ip: IpAddr = "203.0.113.10".parse().unwrap();
        let text = build_linux_core_config(&sample(), ip, 12080).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["inbounds"][0]["stack"], "gvisor");
        assert_eq!(value["inbounds"][0]["dns_mode"], "hijack");
        assert_eq!(value["inbounds"][0]["address"][0], "172.19.0.1/30");
        assert_eq!(value["inbounds"][0]["address"][1], "fdfe:dcba:9876::1/126");
        assert_eq!(
            value["inbounds"][0]["route_exclude_address"][0],
            "203.0.113.10/32"
        );
        assert!(value["inbounds"][0].get("auto_redirect").is_none());
        assert!(value["inbounds"][0].get("strict_route").is_none());
        assert_eq!(value["dns"]["servers"][0]["server"], "1.1.1.1");
        assert_eq!(value["dns"]["servers"][0]["detour"], "mieru-out");
        assert_eq!(value["dns"]["strategy"], "ipv4_only");
        assert_eq!(value["route"]["rules"][0]["protocol"], "dns");
        assert_eq!(value["route"]["rules"][0]["action"], "hijack-dns");
        assert_eq!(value["route"]["final"], "mieru-out");
        assert_eq!(value["route"]["auto_detect_interface"], true);
        assert_eq!(value["outbounds"][0]["server"], "203.0.113.10");
        assert!(!text.contains("example.test"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_sidecar_accepts_hijack_config() {
        let sidecar = sidecar_path().expect("sidecar");
        let ip: IpAddr = "203.0.113.10".parse().unwrap();
        let profile = sample();
        let text = build_linux_core_config(&profile, ip, 12080).unwrap();
        let dir = std::env::temp_dir().join(format!("mieru-linux-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        fsutil::ensure_dir(&dir).unwrap();
        let path = dir.join("config.json");
        fsutil::write_private(&path, text.as_bytes()).unwrap();
        let redactor =
            Redactor::from_secrets(&profile.password, profile.traffic_pattern.as_deref());
        let result = check_core_config(&sidecar, &path, &redactor);
        let _ = std::fs::remove_dir_all(&dir);
        if let Err(err) = &result {
            assert!(!err.contains(&profile.password));
            assert!(!err.contains("GgQIARAK"));
        }
        result.expect("sing-box check");
    }

    #[test]
    fn sidecar_accepts_generated_config() {
        let sidecar = sidecar_path().expect("sidecar");
        let ip: IpAddr = "203.0.113.10".parse().unwrap();
        let profile = sample();
        let text = build_core_config(&profile, ip, 12080).unwrap();
        let dir = std::env::temp_dir().join(format!("mieru-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        fsutil::ensure_dir(&dir).unwrap();
        let path = dir.join("config.json");
        fsutil::write_private(&path, text.as_bytes()).unwrap();
        let redactor =
            Redactor::from_secrets(&profile.password, profile.traffic_pattern.as_deref());
        let result = check_core_config(&sidecar, &path, &redactor);
        let _ = std::fs::remove_dir_all(&dir);
        if let Err(err) = &result {
            assert!(!err.contains(&profile.password));
            assert!(!err.contains("GgQIARAK"));
        }
        result.expect("sing-box check");
    }
}
