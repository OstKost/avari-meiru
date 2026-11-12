use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::time::Duration;

use mieru_shared::install::TUN_DNS;

pub fn encode_query(id: u16, name: &str) -> Result<Vec<u8>, String> {
    if name.is_empty() || name.len() > 253 || name.contains("..") {
        return Err("tunnel check failed".into());
    }
    let mut out = Vec::with_capacity(64);
    out.extend(id.to_be_bytes());
    out.extend(0x0100u16.to_be_bytes());
    out.extend(1u16.to_be_bytes());
    out.extend([0u8; 6]);
    for label in name.split('.') {
        if label.is_empty() || label.len() > 63 || !label.is_ascii() {
            return Err("tunnel check failed".into());
        }
        out.push(label.len() as u8);
        out.extend(label.as_bytes());
    }
    out.push(0);
    out.extend(1u16.to_be_bytes());
    out.extend(1u16.to_be_bytes());
    Ok(out)
}

pub fn first_a(response: &[u8], id: u16) -> Option<Ipv4Addr> {
    if response.len() < 12 || u16::from_be_bytes([response[0], response[1]]) != id {
        return None;
    }
    let flags = u16::from_be_bytes([response[2], response[3]]);
    if flags & 0x000f != 0 {
        return None;
    }
    let qd = u16::from_be_bytes([response[4], response[5]]) as usize;
    let an = u16::from_be_bytes([response[6], response[7]]) as usize;
    if an == 0 {
        return None;
    }
    let mut idx = 12usize;
    for _ in 0..qd {
        idx = skip_name(response, idx)?;
        idx = idx.checked_add(4)?;
    }
    for _ in 0..an {
        idx = skip_name(response, idx)?;
        if idx + 10 > response.len() {
            return None;
        }
        let typ = u16::from_be_bytes([response[idx], response[idx + 1]]);
        let class = u16::from_be_bytes([response[idx + 2], response[idx + 3]]);
        let rdlen = u16::from_be_bytes([response[idx + 8], response[idx + 9]]) as usize;
        idx += 10;
        if idx + rdlen > response.len() {
            return None;
        }
        if typ == 1 && class == 1 && rdlen == 4 {
            return Some(Ipv4Addr::new(
                response[idx],
                response[idx + 1],
                response[idx + 2],
                response[idx + 3],
            ));
        }
        idx += rdlen;
    }
    None
}

fn skip_name(response: &[u8], mut idx: usize) -> Option<usize> {
    let mut jumps = 0;
    loop {
        if idx >= response.len() || jumps > 8 {
            return None;
        }
        let len = response[idx];
        if len == 0 {
            return Some(idx + 1);
        }
        if len & 0xc0 == 0xc0 {
            return Some(idx + 2);
        }
        idx = idx.checked_add(1 + len as usize)?;
        jumps += 1;
    }
}

pub fn query_a(server: &str, name: &str, timeout: Duration) -> Result<Ipv4Addr, String> {
    let id = 0x4d52u16;
    let query = encode_query(id, name)?;
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|_| "tunnel check failed".to_string())?;
    socket
        .set_read_timeout(Some(timeout))
        .map_err(|_| "tunnel check failed".to_string())?;
    socket
        .set_write_timeout(Some(timeout))
        .map_err(|_| "tunnel check failed".to_string())?;
    let dest = format!("{server}:53");
    socket
        .send_to(&query, dest)
        .map_err(|_| "tunnel check failed".to_string())?;
    let mut buf = [0u8; 512];
    let (n, _) = socket
        .recv_from(&mut buf)
        .map_err(|_| "tunnel check failed".to_string())?;
    first_a(&buf[..n], id).ok_or_else(|| "tunnel check failed".to_string())
}

/// Limited check through the tunnel DNS address, then a TCP handshake to port 443.
/// This is not an HTTPS status or an exit-IP proof.
pub fn probe_tunnel() -> Result<(), String> {
    let ip = query_a(TUN_DNS, "example.com", Duration::from_secs(5))?;
    let addr = SocketAddr::new(IpAddr::V4(ip), 443);
    TcpStream::connect_timeout(&addr, Duration::from_secs(8))
        .map(|_| ())
        .map_err(|_| "tunnel check failed".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_record_and_ignores_other_types() {
        let query = encode_query(0x4d52, "example.com").unwrap();
        assert!(query.windows(b"example".len()).any(|w| w == b"example"));
        // header + question example.com + answer A 203.0.113.10
        let mut response = vec![
            0x4d, 0x52, 0x81, 0x80, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x07,
        ];
        response.extend(b"example");
        response.push(0x03);
        response.extend(b"com");
        response.extend([0x00, 0x00, 0x01, 0x00, 0x01]);
        response.extend([0xc0, 0x0c]);
        response.extend([0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x10, 0x00, 0x04]);
        response.extend([203, 0, 113, 10]);
        assert_eq!(
            first_a(&response, 0x4d52),
            Some(Ipv4Addr::new(203, 0, 113, 10))
        );
        assert!(first_a(&response, 1).is_none());
    }
}
