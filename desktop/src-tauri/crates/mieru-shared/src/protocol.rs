use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

use crate::profile::Profile;

pub const MAX_FRAME: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientMsg {
    Hello { session: String },
    Connect { profile: Profile },
    Heartbeat { n: u64 },
    Disconnect,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ServerMsg {
    Hello { pid: u32, start: u64 },
    Status { state: String, message: String },
    Log { line: String },
}

pub fn write_frame(mut writer: impl Write, value: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame is too large",
        ));
    }
    let len = u32::try_from(bytes.len()).map_err(io::Error::other)?;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}

pub fn read_frame(mut reader: impl Read) -> io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len == 0 || len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame length is invalid",
        ));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok(body)
}

pub fn decode_client(bytes: &[u8]) -> Result<ClientMsg, String> {
    serde_json::from_slice(bytes).map_err(|_| "ipc message is invalid".to_string())
}

pub fn decode_server(bytes: &[u8]) -> Result<ServerMsg, String> {
    serde_json::from_slice(bytes).map_err(|_| "ipc message is invalid".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{Profile, Transport};
    use std::io::Cursor;

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
    fn frame_round_trip_keeps_profile_off_the_length_header() {
        let msg = ClientMsg::Connect { profile: sample() };
        let mut buf = Vec::new();
        write_frame(&mut buf, &msg).unwrap();
        assert!(buf
            .windows(b"example-password".len())
            .any(|w| w == b"example-password"));
        let body = read_frame(Cursor::new(buf)).unwrap();
        match decode_client(&body).unwrap() {
            ClientMsg::Connect { profile } => assert_eq!(profile.password, "example-password"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_ops_and_huge_lengths() {
        assert!(decode_client(br#"{"op":"exec","path":"/bin/sh"}"#).is_err());
        let mut huge = vec![0u8; 4];
        huge.copy_from_slice(&1_000_000u32.to_be_bytes());
        assert!(read_frame(Cursor::new(huge)).is_err());
    }
}
