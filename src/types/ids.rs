use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SessionId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AgentId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TaskId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ToolUseId(pub String);

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl SessionId {
    // `new()` mints a fresh value, so `Default` would be misleading.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

/// Maps to: CC `types/ids.ts#asSessionId`.
pub fn as_session_id(id: impl Into<String>) -> SessionId {
    SessionId(id.into())
}

/// Maps to: CC `types/ids.ts#asAgentId`.
pub fn as_agent_id(id: impl Into<String>) -> AgentId {
    AgentId(id.into())
}

/// Maps to: CC `types/ids.ts#toAgentId`.
pub fn to_agent_id(s: &str) -> Option<AgentId> {
    let rest = s.strip_prefix('a')?;
    let suffix = if rest.len() == 16 {
        rest
    } else {
        let (label, suffix) = rest.rsplit_once('-')?;
        if label.is_empty() {
            return None;
        }
        suffix
    };
    (suffix.len() == 16 && suffix.chars().all(|ch| ch.is_ascii_hexdigit()))
        .then(|| AgentId(s.to_string()))
}

impl TaskId {
    // `new()` mints a fresh value, so `Default` would be misleading.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self(format!("task_{}", uuid::Uuid::new_v4().simple()))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn to_agent_id_matches_official_pattern() {
        assert_eq!(
            super::to_agent_id("a0123456789abcdef").map(|id| id.0),
            Some("a0123456789abcdef".to_string())
        );
        assert_eq!(
            super::to_agent_id("acompact-0123456789abcdef").map(|id| id.0),
            Some("acompact-0123456789abcdef".to_string())
        );
        assert_eq!(
            super::to_agent_id("alabel-with-dash-0123456789abcdef").map(|id| id.0),
            Some("alabel-with-dash-0123456789abcdef".to_string())
        );
        assert!(super::to_agent_id("researcher").is_none());
        assert!(super::to_agent_id("a-0123456789abcdef").is_none());
        assert!(super::to_agent_id("a0123456789abcdeg").is_none());
        assert!(super::to_agent_id("a0123456789abcde").is_none());
    }
}
