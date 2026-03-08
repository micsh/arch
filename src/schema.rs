use serde::{Deserialize, Serialize};

/// Root architecture.yaml structure
#[derive(Debug, Serialize, Deserialize)]
pub struct Architecture {
    #[serde(default)]
    pub guidance: Option<String>,
    pub system: System,
    pub containers: Vec<Container>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct System {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Container {
    pub id: String,
    #[serde(default)]
    pub project: Option<String>,
    pub path: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// Per-container YAML structure (architecture/<id>.yaml)
#[derive(Debug, Serialize, Deserialize)]
pub struct ContainerDetail {
    #[serde(default)]
    pub modules: Vec<Module>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Module {
    pub id: String,
    pub file: String,
    #[serde(default)]
    pub owns: Vec<String>,
    #[serde(default)]
    pub boundary: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub must_not_depend: Vec<String>,
    #[serde(default)]
    pub routes: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    #[serde(rename = "type")]
    pub rule_type: String,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<serde_yaml::Value>,
    #[serde(default)]
    pub module: Option<String>,
    #[serde(default)]
    pub modules: Vec<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub constraint: Option<String>,
}

/// Stories YAML structure
#[derive(Debug, Serialize, Deserialize)]
pub struct Stories {
    #[serde(default)]
    pub stories: Vec<Story>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Story {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub flow: Vec<String>,
}
