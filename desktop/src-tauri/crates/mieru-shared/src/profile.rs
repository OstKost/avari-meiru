use std::fs;
use std::net::IpAddr;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::fsutil;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Transport {
    TCP,
    UDP,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub server: String,
    pub server_port: u16,
    pub transport: Transport,
    pub username: String,
    pub password: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traffic_pattern: Option<String>,
}

/// Profile returned to the webview. The password field does not exist here.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileView {
    pub server: String,
    pub server_port: u16,
    pub transport: Transport,
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traffic_pattern: Option<String>,
}

impl Profile {
    pub fn validate(&self) -> Result<(), String> {
        validate_server(&self.server)?;
        if self.server_port == 0 {
            return Err("server_port must be from 1 to 65535".into());
        }
        validate_token("username", &self.username, 256)?;
        validate_password(&self.password)?;
        if let Some(pattern) = &self.traffic_pattern {
            validate_traffic_pattern(pattern)?;
        }
        Ok(())
    }

    pub fn view(&self) -> ProfileView {
        ProfileView {
            server: self.server.clone(),
            server_port: self.server_port,
            transport: self.transport,
            username: self.username.clone(),
            traffic_pattern: self.traffic_pattern.clone(),
        }
    }
}

pub fn validate_server(server: &str) -> Result<(), String> {
    if server.is_empty()
        || server.len() > 253
        || server
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err("server must be a hostname or IP address".into());
    }
    if server.parse::<IpAddr>().is_ok() {
        return Ok(());
    }
    let name = server.strip_suffix('.').unwrap_or(server);
    if name.is_empty() {
        return Err("server must be a hostname or IP address".into());
    }
    for label in name.split('.') {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        {
            return Err("server must be a hostname or IP address".into());
        }
    }
    Ok(())
}

fn validate_token(field: &str, value: &str, max: usize) -> Result<(), String> {
    if value.is_empty() || value.len() > max || value.chars().any(|ch| ch.is_control()) {
        return Err(format!("{field} is invalid"));
    }
    Ok(())
}

fn validate_password(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 4096
        || value
            .chars()
            .any(|ch| ch == '\0' || ch == '\n' || ch == '\r')
    {
        return Err("password is invalid".into());
    }
    Ok(())
}

fn validate_traffic_pattern(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 4096 || !is_standard_base64(value) {
        return Err("traffic_pattern must be standard base64".into());
    }
    Ok(())
}

fn is_standard_base64(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() % 4 != 0 {
        return false;
    }
    let mut padding = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' => {
                if padding > 0 {
                    return false;
                }
            }
            b'=' => {
                padding += 1;
                if padding > 2 || index < bytes.len() - 2 {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

pub fn load(path: &Path) -> Result<Option<Profile>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(|_| "could not read the saved profile".to_string())?;
    let profile: Profile =
        serde_json::from_slice(&bytes).map_err(|_| "saved profile is invalid".to_string())?;
    profile.validate()?;
    Ok(Some(profile))
}

pub fn store(path: &Path, profile: &Profile) -> Result<(), String> {
    profile.validate()?;
    let bytes = serde_json::to_vec_pretty(profile)
        .map_err(|_| "could not store the profile".to_string())?;
    fsutil::write_private(path, &bytes).map_err(|_| "could not store the profile".to_string())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn sample() -> Profile {
        Profile {
            server: "203.0.113.10".into(),
            server_port: 8964,
            transport: Transport::TCP,
            username: "example-user".into(),
            password: "example-password".into(),
            traffic_pattern: Some("GgQIARAK".into()),
        }
    }

    #[test]
    fn view_omits_password() {
        let json = serde_json::to_string(&sample().view()).unwrap();
        assert!(!json.contains("password"));
        assert!(!json.contains("example-password"));
        assert!(json.contains("example-user"));
        assert!(json.contains("GgQIARAK"));
    }

    #[test]
    fn rejects_unknown_fields_and_bad_pattern() {
        let raw = r#"{"server":"203.0.113.10","server_port":1,"transport":"TCP","username":"u","password":"p","extra":1}"#;
        assert!(serde_json::from_str::<Profile>(raw).is_err());
        let mut profile = sample();
        profile.traffic_pattern = Some("not base64".into());
        assert!(profile.validate().is_err());
        profile.traffic_pattern = Some("GgQIARAK".into());
        profile.server_port = 0;
        assert!(profile.validate().is_err());
    }

    #[test]
    fn round_trip_file_is_private() {
        let dir = std::env::temp_dir().join(format!("mieru-profile-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("profile.json");
        store(&path, &sample()).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let loaded = load(&path).unwrap().unwrap();
        assert_eq!(loaded.password, sample().password);
        let _ = fs::remove_dir_all(&dir);
    }
}
