use std::fs;
use std::path::{Path, PathBuf};

use mieru_shared::install::TUN_DNS;

#[derive(Debug)]
pub struct ResolvBackup {
    path: PathBuf,
    symlink_to: Option<PathBuf>,
    bytes: Option<Vec<u8>>,
    replaced: bool,
}

impl ResolvBackup {
    pub fn redirect(path: &Path) -> Result<Self, String> {
        let meta =
            fs::symlink_metadata(path).map_err(|_| "could not read system DNS".to_string())?;
        let symlink_to = if meta.file_type().is_symlink() {
            Some(fs::read_link(path).map_err(|_| "could not read system DNS".to_string())?)
        } else if meta.is_file() {
            None
        } else {
            return Err("could not read system DNS".into());
        };
        let bytes = if symlink_to.is_none() {
            Some(fs::read(path).map_err(|_| "could not read system DNS".to_string())?)
        } else {
            None
        };
        fs::remove_file(path)
            .map_err(|_| "could not point system DNS at the tunnel".to_string())?;
        let body = format!("nameserver {TUN_DNS}\noptions timeout:2 attempts:1\n");
        if let Err(err) = fs::write(path, body.as_bytes()) {
            if let Some(target) = &symlink_to {
                let _ = std::os::unix::fs::symlink(target, path);
            } else if let Some(bytes) = &bytes {
                let _ = fs::write(path, bytes);
            }
            let _ = err;
            return Err("could not point system DNS at the tunnel".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o644));
        }
        Ok(Self {
            path: path.to_path_buf(),
            symlink_to,
            bytes,
            replaced: true,
        })
    }

    pub fn restore(&mut self) -> Result<(), String> {
        if !self.replaced {
            return Ok(());
        }
        let _ = fs::remove_file(&self.path);
        if let Some(target) = &self.symlink_to {
            std::os::unix::fs::symlink(target, &self.path)
                .map_err(|_| "could not restore system DNS".to_string())?;
        } else if let Some(bytes) = &self.bytes {
            fs::write(&self.path, bytes).map_err(|_| "could not restore system DNS".to_string())?;
        }
        self.replaced = false;
        Ok(())
    }
}

impl Drop for ResolvBackup {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

/// Best-effort systemd-resolved hook. The iface name is already constrained
/// to `tun` plus digits. Failure here does not replace the resolv.conf path.
pub fn resolvectl_dns(iface: &str) -> Result<(), String> {
    if !mieru_shared::install::is_tun_name(iface) {
        return Err("unexpected tun name".into());
    }
    if !Path::new("/usr/bin/resolvectl").is_file() {
        return Ok(());
    }
    let dns = std::process::Command::new("/usr/bin/resolvectl")
        .args(["dns", iface, TUN_DNS])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|_| "could not point system DNS at the tunnel".to_string())?;
    if !dns.status.success() {
        return Err("could not point system DNS at the tunnel".into());
    }
    let domain = match std::process::Command::new("/usr/bin/resolvectl")
        .args(["domain", iface, "~."])
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(output) => output,
        Err(_) => {
            resolvectl_revert(iface);
            return Err("could not point system DNS at the tunnel".into());
        }
    };
    if domain.status.success() {
        Ok(())
    } else {
        // The interface DNS setting is already applied. Drop it before the
        // session records the iface, or a later failure would leave it behind.
        resolvectl_revert(iface);
        Err("could not point system DNS at the tunnel".into())
    }
}

pub fn resolvectl_revert(iface: &str) {
    if !mieru_shared::install::is_tun_name(iface) || !Path::new("/usr/bin/resolvectl").is_file() {
        return;
    }
    let _ = std::process::Command::new("/usr/bin/resolvectl")
        .args(["revert", iface])
        .stdin(std::process::Stdio::null())
        .output();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolv_redirect_restores_file_and_symlink() {
        let dir = std::env::temp_dir().join(format!("mieru-resolv-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("resolv.conf");
        fs::write(&file, b"nameserver 127.0.0.53\n").unwrap();
        let mut backup = ResolvBackup::redirect(&file).unwrap();
        let body = fs::read_to_string(&file).unwrap();
        assert!(body.contains("nameserver 172.19.0.2"));
        assert!(!body.contains("example-password"));
        backup.restore().unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"nameserver 127.0.0.53\n");

        let link = dir.join("resolv-link.conf");
        std::os::unix::fs::symlink("stub-resolv.conf", &link).unwrap();
        let mut linked = ResolvBackup::redirect(&link).unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
                == false
        );
        linked.restore().unwrap();
        assert_eq!(
            fs::read_link(&link).unwrap(),
            PathBuf::from("stub-resolv.conf")
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
