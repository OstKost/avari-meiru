use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VpnState {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VpnStatus {
    pub state: VpnState,
    pub message: String,
    pub since: String,
}

impl VpnStatus {
    pub fn new(state: VpnState, message: impl Into<String>) -> Self {
        let message = match state {
            VpnState::Error => message.into(),
            _ => String::new(),
        };
        Self {
            state,
            message,
            since: now_rfc3339(),
        }
    }
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_error_status_has_empty_message() {
        let status = VpnStatus::new(VpnState::Connected, "ignored");
        assert_eq!(status.state, VpnState::Connected);
        assert_eq!(status.message, "");
        assert!(status.since.ends_with('Z'));
    }
}
