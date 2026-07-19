use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::detect::Agent;

const MAX_SIDEBAR_ROWS: usize = 16;
const MAX_SIDEBAR_TOKENS_PER_ROW: usize = 16;
const DEFAULT_SIDEBAR_ROW_GAP: u16 = 0;
const DEFAULT_CUSTOM_SECTION_MAX_ROWS: u16 = 12;
const MIN_CUSTOM_SECTION_MAX_ROWS: u16 = 1;
const MAX_CUSTOM_SECTION_MAX_ROWS: u16 = 24;

fn deserialize_sidebar_rows<'de, D, T>(deserializer: D) -> Result<Vec<Vec<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    let rows = Vec::<Vec<T>>::deserialize(deserializer)?;
    validate_sidebar_rows(&rows).map_err(serde::de::Error::custom)?;
    Ok(rows)
}

fn validate_sidebar_rows<T>(rows: &[Vec<T>]) -> Result<(), String> {
    if rows.len() > MAX_SIDEBAR_ROWS {
        return Err(format!(
            "sidebar layouts may contain at most {MAX_SIDEBAR_ROWS} rows"
        ));
    }
    if rows
        .iter()
        .any(|row| row.len() > MAX_SIDEBAR_TOKENS_PER_ROW)
    {
        return Err(format!(
            "sidebar rows may contain at most {MAX_SIDEBAR_TOKENS_PER_ROW} tokens"
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentSidebarToken {
    StateIcon,
    StateText,
    Workspace,
    Tab,
    Pane,
    Agent,
    TerminalTitle,
    TerminalTitleStripped,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceSidebarToken {
    StateIcon,
    StateText,
    Workspace,
    Branch,
    GitStatus,
    Custom(String),
}

fn parse_sidebar_token<'de, D, T>(deserializer: D, builtins: &[(&str, T)]) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Clone + From<String>,
{
    let value = String::deserialize(deserializer)?;
    if let Some((_, token)) = builtins.iter().find(|(name, _)| *name == value) {
        return Ok(token.clone());
    }
    let Some(name) = value.strip_prefix('$') else {
        return Err(serde::de::Error::custom(format!(
            "unknown sidebar token `{value}`; custom tokens must start with `$`"
        )));
    };
    if name.is_empty()
        || name.len() > 32
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        return Err(serde::de::Error::custom(format!(
            "invalid custom sidebar token `{value}`"
        )));
    }
    Ok(T::from(name.to_string()))
}

impl Serialize for AgentSidebarToken {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::StateIcon => serializer.serialize_str("state_icon"),
            Self::StateText => serializer.serialize_str("state_text"),
            Self::Workspace => serializer.serialize_str("workspace"),
            Self::Tab => serializer.serialize_str("tab"),
            Self::Pane => serializer.serialize_str("pane"),
            Self::Agent => serializer.serialize_str("agent"),
            Self::TerminalTitle => serializer.serialize_str("terminal_title"),
            Self::TerminalTitleStripped => serializer.serialize_str("terminal_title_stripped"),
            Self::Custom(name) => serializer.serialize_str(&format!("${name}")),
        }
    }
}

impl From<String> for AgentSidebarToken {
    fn from(value: String) -> Self {
        Self::Custom(value)
    }
}

impl<'de> Deserialize<'de> for AgentSidebarToken {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        parse_sidebar_token(
            deserializer,
            &[
                ("state_icon", Self::StateIcon),
                ("state_text", Self::StateText),
                ("workspace", Self::Workspace),
                ("tab", Self::Tab),
                ("pane", Self::Pane),
                ("agent", Self::Agent),
                ("terminal_title", Self::TerminalTitle),
                ("terminal_title_stripped", Self::TerminalTitleStripped),
            ],
        )
    }
}

impl Serialize for SpaceSidebarToken {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::StateIcon => serializer.serialize_str("state_icon"),
            Self::StateText => serializer.serialize_str("state_text"),
            Self::Workspace => serializer.serialize_str("workspace"),
            Self::Branch => serializer.serialize_str("branch"),
            Self::GitStatus => serializer.serialize_str("git_status"),
            Self::Custom(name) => serializer.serialize_str(&format!("${name}")),
        }
    }
}

impl From<String> for SpaceSidebarToken {
    fn from(value: String) -> Self {
        Self::Custom(value)
    }
}

impl<'de> Deserialize<'de> for SpaceSidebarToken {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        parse_sidebar_token(
            deserializer,
            &[
                ("state_icon", Self::StateIcon),
                ("state_text", Self::StateText),
                ("workspace", Self::Workspace),
                ("branch", Self::Branch),
                ("git_status", Self::GitStatus),
            ],
        )
    }
}

type AgentSidebarRows = Vec<Vec<AgentSidebarToken>>;
type SpaceSidebarRows = Vec<Vec<SpaceSidebarToken>>;

fn deserialize_rows_by_agent<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, AgentSidebarRows>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let rows_by_agent = BTreeMap::<String, AgentSidebarRows>::deserialize(deserializer)?;
    for (id, rows) in &rows_by_agent {
        if crate::detect::parse_canonical_agent_label(id).is_none() {
            return Err(serde::de::Error::custom(format!(
                "unknown canonical agent id `{id}` in sidebar rows_by_agent"
            )));
        }
        validate_sidebar_rows(rows).map_err(serde::de::Error::custom)?;
    }
    Ok(rows_by_agent)
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct AgentsSidebarConfig {
    #[serde(deserialize_with = "deserialize_sidebar_rows")]
    pub rows: AgentSidebarRows,
    #[serde(default, deserialize_with = "deserialize_rows_by_agent")]
    pub rows_by_agent: BTreeMap<String, AgentSidebarRows>,
    pub row_gap: u16,
}

impl AgentsSidebarConfig {
    pub(crate) fn rows_for_agent(&self, agent: Option<Agent>) -> &AgentSidebarRows {
        agent
            .and_then(|agent| self.rows_by_agent.get(crate::detect::agent_label(agent)))
            .unwrap_or(&self.rows)
    }
}

impl Default for AgentsSidebarConfig {
    fn default() -> Self {
        Self {
            rows: vec![
                vec![
                    AgentSidebarToken::StateIcon,
                    AgentSidebarToken::Workspace,
                    AgentSidebarToken::Tab,
                ],
                vec![AgentSidebarToken::Agent],
            ],
            rows_by_agent: BTreeMap::new(),
            row_gap: DEFAULT_SIDEBAR_ROW_GAP,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct SpacesSidebarConfig {
    #[serde(deserialize_with = "deserialize_sidebar_rows")]
    pub rows: SpaceSidebarRows,
    pub row_gap: u16,
}

impl Default for SpacesSidebarConfig {
    fn default() -> Self {
        Self {
            rows: vec![
                vec![SpaceSidebarToken::StateIcon, SpaceSidebarToken::Workspace],
                vec![SpaceSidebarToken::Branch, SpaceSidebarToken::GitStatus],
            ],
            row_gap: DEFAULT_SIDEBAR_ROW_GAP,
        }
    }
}

fn default_custom_section_max_rows() -> u16 {
    DEFAULT_CUSTOM_SECTION_MAX_ROWS
}

fn deserialize_custom_section_max_rows<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(u16::deserialize(deserializer)?
        .clamp(MIN_CUSTOM_SECTION_MAX_ROWS, MAX_CUSTOM_SECTION_MAX_ROWS))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarSectionPlacement {
    #[default]
    BelowAgents,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct CustomSidebarSectionConfig {
    pub id: String,
    pub title: Option<String>,
    /// Pane metadata token whose focused value marks matching bar rows.
    pub highlight_token: Option<String>,
    #[serde(
        default = "default_custom_section_max_rows",
        deserialize_with = "deserialize_custom_section_max_rows"
    )]
    pub max_rows: u16,
    pub placement: SidebarSectionPlacement,
}

impl Default for CustomSidebarSectionConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: None,
            highlight_token: None,
            max_rows: default_custom_section_max_rows(),
            placement: SidebarSectionPlacement::BelowAgents,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct SidebarConfig {
    pub agents: AgentsSidebarConfig,
    pub spaces: SpacesSidebarConfig,
    pub sections: Vec<CustomSidebarSectionConfig>,
}

impl SidebarConfig {
    pub(crate) fn resolved_sections(&self) -> Vec<CustomSidebarSectionConfig> {
        self.validated_sections().0
    }

    pub(crate) fn section_diagnostics(&self) -> Vec<String> {
        self.validated_sections().1
    }

    fn validated_sections(&self) -> (Vec<CustomSidebarSectionConfig>, Vec<String>) {
        let mut sections = Vec::new();
        let mut diagnostics = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (index, section) in self.sections.iter().enumerate() {
            if !sidebar_section_id_is_valid(&section.id) {
                diagnostics.push(format!(
                    "invalid sidebar section id: ui.sidebar.sections[{index}].id = {:?}; ignoring section",
                    section.id
                ));
                continue;
            }
            if !seen.insert(section.id.as_str()) {
                diagnostics.push(format!(
                    "duplicate sidebar section id: ui.sidebar.sections[{index}].id = {:?}; ignoring section",
                    section.id
                ));
                continue;
            }
            let mut section = section.clone();
            if section
                .highlight_token
                .as_deref()
                .is_some_and(|token| !sidebar_section_id_is_valid(token))
            {
                diagnostics.push(format!(
                    "invalid sidebar section highlight token: ui.sidebar.sections[{index}].highlight_token = {:?}; disabling row highlighting",
                    section.highlight_token
                ));
                section.highlight_token = None;
            }
            sections.push(section);
        }
        (sections, diagnostics)
    }
}

fn sidebar_section_id_is_valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_compact_agent_and_existing_space_layouts() {
        let config = SidebarConfig::default();
        assert_eq!(
            config.agents.rows,
            vec![
                vec![
                    AgentSidebarToken::StateIcon,
                    AgentSidebarToken::Workspace,
                    AgentSidebarToken::Tab,
                ],
                vec![AgentSidebarToken::Agent],
            ]
        );
        assert!(config.agents.rows_by_agent.is_empty());
        assert_eq!(config.agents.row_gap, 0);
        assert_eq!(
            config.spaces.rows,
            vec![
                vec![SpaceSidebarToken::StateIcon, SpaceSidebarToken::Workspace],
                vec![SpaceSidebarToken::Branch, SpaceSidebarToken::GitStatus],
            ]
        );
        assert_eq!(config.spaces.row_gap, 0);
        assert!(config.sections.is_empty());
    }

    #[test]
    fn parses_builtin_and_arbitrary_custom_tokens() {
        let config: crate::config::Config = toml::from_str(
            r#"
[ui.sidebar.agents]
rows = [["state_icon", "workspace"], ["state_text", "agent", "$summary"], ["terminal_title", "terminal_title_stripped", "$terminal_title"]]
row_gap = 1

[ui.sidebar.agents.rows_by_agent]
claude = [["terminal_title_stripped"], ["agent", "$model"]]

[ui.sidebar.spaces]
rows = [["workspace"], ["$jj_status"]]
row_gap = 3
"#,
        )
        .expect("sidebar token config");

        assert_eq!(
            config.ui.sidebar.agents.rows[1],
            vec![
                AgentSidebarToken::StateText,
                AgentSidebarToken::Agent,
                AgentSidebarToken::Custom("summary".into()),
            ]
        );
        assert_eq!(
            config.ui.sidebar.agents.rows[2],
            vec![
                AgentSidebarToken::TerminalTitle,
                AgentSidebarToken::TerminalTitleStripped,
                AgentSidebarToken::Custom("terminal_title".into()),
            ]
        );
        assert_eq!(
            config.ui.sidebar.agents.rows_by_agent["claude"],
            vec![
                vec![AgentSidebarToken::TerminalTitleStripped],
                vec![
                    AgentSidebarToken::Agent,
                    AgentSidebarToken::Custom("model".into()),
                ],
            ]
        );
        assert_eq!(config.ui.sidebar.agents.row_gap, 1);
        assert_eq!(
            config.ui.sidebar.spaces.rows[1],
            vec![SpaceSidebarToken::Custom("jj_status".into())]
        );
        assert_eq!(config.ui.sidebar.spaces.row_gap, 3);
    }

    #[test]
    fn rejects_unknown_bare_and_malformed_custom_tokens() {
        for token in ["summary", "$", "$bad.name"] {
            let input = format!("[ui.sidebar.agents]\\nrows = [[\"{token}\"]]\\n");
            assert!(toml::from_str::<crate::config::Config>(&input).is_err());
        }
    }

    #[test]
    fn rejects_oversized_sidebar_layouts() {
        let too_many_rows = std::iter::repeat_n("[\"agent\"]", MAX_SIDEBAR_ROWS + 1)
            .collect::<Vec<_>>()
            .join(",");
        let input = format!("[ui.sidebar.agents]\nrows = [{too_many_rows}]\n");
        assert!(toml::from_str::<crate::config::Config>(&input).is_err());

        let too_many_tokens = std::iter::repeat_n("\"workspace\"", MAX_SIDEBAR_TOKENS_PER_ROW + 1)
            .collect::<Vec<_>>()
            .join(",");
        let input = format!("[ui.sidebar.spaces]\nrows = [[{too_many_tokens}]]\n");
        assert!(toml::from_str::<crate::config::Config>(&input).is_err());

        let input = format!("[ui.sidebar.agents.rows_by_agent]\nclaude = [{too_many_rows}]\n");
        assert!(toml::from_str::<crate::config::Config>(&input).is_err());
    }

    #[test]
    fn accepts_every_canonical_agent_override_key() {
        let agents = [
            Agent::Pi,
            Agent::Claude,
            Agent::Codex,
            Agent::Gemini,
            Agent::Cursor,
            Agent::Devin,
            Agent::Antigravity,
            Agent::Cline,
            Agent::Omp,
            Agent::Mastracode,
            Agent::OpenCode,
            Agent::GithubCopilot,
            Agent::Kimi,
            Agent::Kiro,
            Agent::Droid,
            Agent::Amp,
            Agent::Grok,
            Agent::Hermes,
            Agent::Kilo,
            Agent::Qodercli,
            Agent::Maki,
        ];
        let entries = agents
            .iter()
            .map(|agent| format!("{} = [[\"agent\"]]", crate::detect::agent_label(*agent)))
            .collect::<Vec<_>>()
            .join("\n");
        let input = format!("[ui.sidebar.agents.rows_by_agent]\n{entries}\n");
        let config: crate::config::Config = toml::from_str(&input).expect("canonical keys");

        assert_eq!(config.ui.sidebar.agents.rows_by_agent.len(), agents.len());
    }

    #[test]
    fn rejects_alias_case_whitespace_and_unknown_override_keys() {
        for key in ["claude-code", "Claude", "' claude '", "unknown"] {
            let input = format!("[ui.sidebar.agents.rows_by_agent]\n{key} = [[\"agent\"]]\n");
            assert!(
                toml::from_str::<crate::config::Config>(&input).is_err(),
                "accepted key {key:?}"
            );
        }
    }
    #[test]
    fn parses_custom_sections_and_clamps_max_rows() {
        let config: crate::config::Config = toml::from_str(
            r#"
[[ui.sidebar.sections]]
id = "build"
title = "Build status"
highlight_token = "vault_broker"
max_rows = 99
placement = "below_agents"

[[ui.sidebar.sections]]
id = "deploy"
placement = "below_agents"

[[ui.sidebar.sections]]
id = "minimum"
max_rows = 0
placement = "below_agents"
"#,
        )
        .expect("custom sections");

        assert_eq!(
            config.ui.sidebar.sections,
            vec![
                CustomSidebarSectionConfig {
                    id: "build".into(),
                    title: Some("Build status".into()),
                    highlight_token: Some("vault_broker".into()),
                    max_rows: MAX_CUSTOM_SECTION_MAX_ROWS,
                    placement: SidebarSectionPlacement::BelowAgents,
                },
                CustomSidebarSectionConfig {
                    id: "deploy".into(),
                    title: None,
                    highlight_token: None,
                    max_rows: DEFAULT_CUSTOM_SECTION_MAX_ROWS,
                    placement: SidebarSectionPlacement::BelowAgents,
                },
                CustomSidebarSectionConfig {
                    id: "minimum".into(),
                    title: None,
                    highlight_token: None,
                    max_rows: MIN_CUSTOM_SECTION_MAX_ROWS,
                    placement: SidebarSectionPlacement::BelowAgents,
                },
            ]
        );
        assert_eq!(config.ui.sidebar.resolved_sections().len(), 3);
    }

    #[test]
    fn bad_and_duplicate_section_ids_are_diagnostic_and_ignored() {
        let config: crate::config::Config = toml::from_str(
            r#"
[[ui.sidebar.sections]]
id = "bad.id"
placement = "below_agents"

[[ui.sidebar.sections]]
id = "build"
placement = "below_agents"

[[ui.sidebar.sections]]
id = "build"
title = "duplicate"
placement = "below_agents"
"#,
        )
        .expect("section config remains parseable");

        let resolved = config.ui.sidebar.resolved_sections();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].id, "build");
        let diagnostics = config.collect_diagnostics();
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("invalid sidebar section id")));
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("duplicate sidebar section id")));
    }

    #[test]
    fn invalid_section_highlight_token_is_diagnostic_and_disabled() {
        let config: crate::config::Config = toml::from_str(
            r#"
[[ui.sidebar.sections]]
id = "usage"
highlight_token = "vault.broker"
placement = "below_agents"
"#,
        )
        .expect("invalid highlight token remains parseable");

        let resolved = config.ui.sidebar.resolved_sections();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].id, "usage");
        assert_eq!(resolved[0].highlight_token, None);
        assert!(config
            .collect_diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.contains("invalid sidebar section highlight token")));
    }

    #[test]
    fn section_placement_rejects_unknown_variants() {
        let input = r#"
[[ui.sidebar.sections]]
id = "build"
placement = "above_agents"
"#;
        assert!(toml::from_str::<crate::config::Config>(input).is_err());
    }
}
