use glob::Pattern;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Check if a relative path matches any of the ignore patterns.
pub fn is_ignored(relative_path: &str, ignore_patterns: &[Pattern]) -> bool {
    let normalized = relative_path.replace('\\', "/");
    ignore_patterns.iter().any(|p| p.matches(&normalized))
}

/// Compile ignore glob strings into patterns.
pub fn compile_ignore_patterns(patterns: &[String]) -> Vec<Pattern> {
    patterns
        .iter()
        .filter_map(|p| Pattern::new(p).ok())
        .collect()
}

/// Find architecture.yaml — checks architecture/architecture.yaml first, then root.
pub fn find_arch_yaml() -> Result<PathBuf, String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let nested = root.join("architecture").join("architecture.yaml");
    if nested.exists() {
        return Ok(nested);
    }
    let flat = root.join("architecture.yaml");
    if flat.exists() {
        return Ok(flat);
    }
    Err("architecture.yaml not found. Run `arch init` first.".into())
}

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
    #[serde(default)]
    pub ignore: Vec<String>,
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

/// Entry-point filenames that imply directory ownership.
/// When a module's `file` points to one of these, all source files
/// in the same directory are considered covered by that module.
pub const ENTRY_POINT_FILES: &[&str] = &[
    "__init__.py",
    "mod.rs",
    "lib.rs",
    "index.ts",
    "index.tsx",
    "index.js",
    "index.jsx",
];

/// Check whether a module file path ends with a recognized entry-point filename.
pub fn is_entry_point(file: &str) -> bool {
    let normalized = file.replace('\\', "/");
    let file_name = normalized.rsplit('/').next().unwrap_or(&normalized);
    ENTRY_POINT_FILES.contains(&file_name)
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
