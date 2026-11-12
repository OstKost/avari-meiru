#[derive(Clone)]
pub struct Redactor {
    secrets: Vec<String>,
}

impl Redactor {
    pub fn from_secrets(password: &str, traffic_pattern: Option<&str>) -> Self {
        let mut secrets = Vec::new();
        if !password.is_empty() {
            secrets.push(password.to_string());
        }
        if let Some(pattern) = traffic_pattern {
            if !pattern.is_empty() {
                secrets.push(pattern.to_string());
            }
        }
        Self { secrets }
    }

    pub fn apply(&self, text: &str) -> String {
        let mut out = text.to_string();
        for secret in &self.secrets {
            if !secret.is_empty() {
                out = out.replace(secret, "[redacted]");
            }
        }
        out
    }

    pub fn last_line(&self, text: &str, limit: usize) -> String {
        let redacted = self.apply(text);
        let line = redacted
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim();
        truncate(line, limit)
    }
}

pub fn truncate(text: &str, limit: usize) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if out.len() + ch.len_utf8() > limit {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_password_and_pattern() {
        let redactor = Redactor::from_secrets("example-password", Some("GgQIARAK"));
        let text = "auth example-password pattern GgQIARAK";
        let cleaned = redactor.apply(text);
        assert!(!cleaned.contains("example-password"));
        assert!(!cleaned.contains("GgQIARAK"));
        assert!(cleaned.contains("[redacted]"));
    }
}
