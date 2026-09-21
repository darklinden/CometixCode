//! Maps to: CC `utils/statusNoticeHelpers.ts`.

pub const AGENT_DESCRIPTIONS_THRESHOLD: u64 = 15_000;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentDefinitionSnapshot {
    /// Maps to CC `AgentDefinition.agentType`.
    pub agent_type: String,
    /// Maps to CC `AgentDefinition.whenToUse`.
    pub when_to_use: String,
    /// Maps to CC `AgentDefinition.source`.
    pub source: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentDefinitionsSnapshot {
    /// Maps to CC `AgentDefinitionsResult.activeAgents`.
    pub active_agents: Vec<AgentDefinitionSnapshot>,
}

/// Maps to CC `getAgentDescriptionsTotalTokens(...)`.
pub fn get_agent_descriptions_total_tokens(
    agent_definitions: Option<&AgentDefinitionsSnapshot>,
) -> u64 {
    let Some(agent_definitions) = agent_definitions else {
        return 0;
    };

    agent_definitions
        .active_agents
        .iter()
        .filter(|agent| agent.source != "built-in")
        .map(|agent| {
            crate::services::token_estimation::rough_token_count_estimation(&format!(
                "{}: {}",
                agent.agent_type, agent.when_to_use
            )) as u64
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_description_tokens_skip_built_in_agents() {
        let definitions = AgentDefinitionsSnapshot {
            active_agents: vec![
                AgentDefinitionSnapshot {
                    agent_type: "general-purpose".to_string(),
                    when_to_use: "Built in".to_string(),
                    source: "built-in".to_string(),
                },
                AgentDefinitionSnapshot {
                    agent_type: "reviewer".to_string(),
                    when_to_use: "Review code changes".to_string(),
                    source: "project".to_string(),
                },
            ],
        };

        // CC `statusNoticeHelpers.ts` sums `roughTokenCountEstimation` over
        // non-built-in agents, and that is `Math.round(content.length / 4)`
        // (`tokenEstimation.ts:203-208`). Only "reviewer" counts here, and
        // `"reviewer: Review code changes"` is 29 UTF-16 units, so the source
        // yields `Math.round(7.25) = 7`. The Rust `(len + 2) / 4` integer form
        // is the same function; the old expectation of 8 was simply wrong.
        assert_eq!(get_agent_descriptions_total_tokens(Some(&definitions)), 7);
        assert_eq!(get_agent_descriptions_total_tokens(None), 0);
    }
}
