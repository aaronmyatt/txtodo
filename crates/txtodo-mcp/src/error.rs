//! One error type carried through every [`crate::backend::McpBackend`] method (mcp-server-tools
//! notes.md "Structured errors"), mapped into MCP's JSON-RPC error result by [`schema`](crate::schema).

use serde_json::{Value, json};

/// Why an [`McpBackend`](crate::backend::McpBackend) call failed. `spec_rule` names a rule id in
/// `specs/todotxt.abnf` / `specs/ref-directories.md` so an agent that writes `(a) task` learns why
/// (design §6.3); `line` is the offending 1-based line when known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpError {
    /// A short machine-facing code, e.g. `"invalid_params"`, `"not_found"`, `"stale"`, `"denied"`.
    pub code: &'static str,
    /// Human-readable detail.
    pub message: String,
    /// The offending 1-based line, when the error is line-specific.
    pub line: Option<u32>,
    /// A rule id in `specs/todotxt.abnf` or `specs/ref-directories.md`, when applicable. Owned:
    /// the daemon owns the list of rules (`MutationError::spec_rule`) and sends one as metadata;
    /// this crate passes it through instead of allow-listing the ones it has heard of (task
    /// mcp-refusal-metadata).
    pub spec_rule: Option<String>,
}

impl McpError {
    /// A client-supplied argument was invalid (bad shape, missing a required alternative, etc.).
    pub fn invalid_params(message: impl Into<String>) -> McpError {
        McpError {
            code: "invalid_params",
            message: message.into(),
            line: None,
            spec_rule: None,
        }
    }

    /// The referenced task, file, or resource does not exist.
    pub fn not_found(message: impl Into<String>) -> McpError {
        McpError {
            code: "not_found",
            message: message.into(),
            line: None,
            spec_rule: None,
        }
    }

    /// A destructive tool was called without `confirm: true` (design §6.3 invariant).
    pub fn confirm_required(message: impl Into<String>) -> McpError {
        McpError {
            code: "confirm_required",
            message: message.into(),
            line: None,
            spec_rule: None,
        }
    }

    /// The daemon (over gRPC) reported a failure.
    pub fn daemon(message: impl Into<String>) -> McpError {
        McpError {
            code: "daemon",
            message: message.into(),
            line: None,
            spec_rule: None,
        }
    }

    /// Attaches the offending line number.
    #[must_use]
    pub fn with_line(mut self, line: u32) -> McpError {
        self.line = Some(line);
        self
    }

    /// Attaches a spec rule id.
    #[must_use]
    pub fn with_spec_rule(mut self, rule: impl Into<String>) -> McpError {
        self.spec_rule = Some(rule.into());
        self
    }

    /// `{ code, message, line?, spec_rule? }`, the shape carried in the MCP error's `data` field.
    fn to_json(&self) -> Value {
        let mut obj = json!({ "code": self.code, "message": self.message });
        if let Some(line) = self.line {
            obj["line"] = json!(line);
        }
        if let Some(rule) = &self.spec_rule {
            obj["spec_rule"] = json!(rule);
        }
        obj
    }
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for McpError {}

impl From<McpError> for rmcp::ErrorData {
    fn from(err: McpError) -> rmcp::ErrorData {
        let data = Some(err.to_json());
        match err.code {
            "not_found" => rmcp::ErrorData::resource_not_found(err.message.clone(), data),
            "invalid_params" | "confirm_required" => {
                rmcp::ErrorData::invalid_params(err.message.clone(), data)
            }
            _ => rmcp::ErrorData::internal_error(err.message.clone(), data),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_carries_line_and_spec_rule_when_set() {
        let err = McpError::invalid_params("bad priority")
            .with_line(4)
            .with_spec_rule("priority");
        let json = err.to_json();
        assert_eq!(json["code"], "invalid_params");
        assert_eq!(json["line"], 4);
        assert_eq!(json["spec_rule"], "priority");
    }

    #[test]
    fn json_omits_absent_line_and_spec_rule() {
        let json = McpError::not_found("no such task").to_json();
        assert!(json.get("line").is_none());
        assert!(json.get("spec_rule").is_none());
    }

    #[test]
    fn maps_to_the_matching_rmcp_error_code() {
        let not_found: rmcp::ErrorData = McpError::not_found("x").into();
        let invalid: rmcp::ErrorData = McpError::invalid_params("x").into();
        let daemon: rmcp::ErrorData = McpError::daemon("x").into();
        assert_ne!(not_found.code, invalid.code);
        assert_ne!(invalid.code, daemon.code);
    }
}
