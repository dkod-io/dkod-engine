//! Output formatting for dk CLI.
//!
//! Human mode: colored, formatted text.
//! JSON mode (`--json`): machine-readable JSON for agents.

use serde::Serialize;

#[derive(Clone, Copy)]
pub struct Output {
    json: bool,
}

impl Output {
    pub fn new(json: bool) -> Self {
        Self { json }
    }

    pub fn human() -> Self {
        Self { json: false }
    }

    pub fn json() -> Self {
        Self { json: true }
    }

    pub fn is_json(&self) -> bool {
        self.json
    }

    pub fn print_json<T: Serialize>(&self, value: &T) {
        if self.json {
            if let Ok(json) = serde_json::to_string(value) {
                println!("{json}");
            }
        }
    }

    pub fn format(&self, value: &serde_json::Value) -> String {
        serde_json::to_string(value).unwrap_or_default()
    }

    pub fn error(&self, msg: &str) {
        if self.json {
            println!(r#"{{"error":"{}"}}"#, msg.replace('"', r#"\""#));
        } else {
            eprintln!("error: {msg}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_mode_outputs_valid_json() {
        let out = Output::json();
        let result = out.format(&serde_json::json!({"key": "value"}));
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["key"], "value");
    }

    #[test]
    fn human_mode_returns_string_as_is() {
        let out = Output::human();
        assert!(!out.is_json());
    }
}
