use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleDefinition {
    EnsureFile(EnsureFileConfig),
    EnsureJsonKey(EnsureJsonKeyConfig),
    EnsureYamlKey(EnsureYamlKeyConfig),
    FileMatches(FileMatchesConfig),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnsureFileConfig {
    pub path: String,
    pub content: Option<String>,
    #[serde(default)]
    pub mode: EnsureFileMode,
}

#[derive(Debug, Default, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnsureFileMode {
    #[default]
    CreateIfMissing,
    ExactMatch,
    Contains,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnsureJsonKeyConfig {
    pub path: String,
    pub key: String,
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    pub mode: JsonKeyMode,
}

#[derive(Debug, Default, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JsonKeyMode {
    #[default]
    CheckOnly,
    Enforce,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnsureYamlKeyConfig {
    pub path: String,
    pub key: String,
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    pub mode: JsonKeyMode,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileMatchesConfig {
    pub path: String,
    pub pattern: String,
}
