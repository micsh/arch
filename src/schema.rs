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
    /// Additional files covered by this module (for multi-project grouping).
    /// Each file gets the same coverage and drift treatment as `file`.
    #[serde(default)]
    pub files: Vec<String>,
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

impl Module {
    /// All files owned by this module (primary + additional).
    pub fn all_files(&self) -> Vec<&str> {
        let mut result = vec![self.file.as_str()];
        for f in &self.files {
            result.push(f.as_str());
        }
        result
    }
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
    #[serde(default)]
    pub pattern: Option<String>,
}

/// Entry-point filenames that imply directory ownership.
/// When a module's `file` points to one of these, all source files
/// in the directory tree are considered covered by that module.
pub const ENTRY_POINT_FILES: &[&str] = &[
    "__init__.py",
    "mod.rs",
    "lib.rs",
    "index.ts",
    "index.tsx",
    "index.js",
    "index.jsx",
];

/// Project file extensions that imply directory ownership.
/// When a module's `file` points to a project file, all source files
/// in the directory tree are considered covered by that module.
pub const PROJECT_FILE_EXTENSIONS: &[&str] = &[
    "csproj", "fsproj", "vbproj",
];

/// Check whether a module file path ends with a recognized entry-point filename.
pub fn is_entry_point(file: &str) -> bool {
    let normalized = file.replace('\\', "/");
    let file_name = normalized.rsplit('/').next().unwrap_or(&normalized);
    ENTRY_POINT_FILES.contains(&file_name)
}

/// Check whether a module file path ends with a project file extension.
pub fn is_project_file(file: &str) -> bool {
    let normalized = file.replace('\\', "/");
    let file_name = normalized.rsplit('/').next().unwrap_or(&normalized);
    file_name
        .rsplit('.')
        .next()
        .map(|ext| PROJECT_FILE_EXTENSIONS.contains(&ext))
        .unwrap_or(false)
}

/// Check whether a module file implies directory ownership (entry point or project file).
pub fn is_directory_owner(file: &str) -> bool {
    is_entry_point(file) || is_project_file(file)
}

/// Directories to always skip during file scanning (build artifacts, caches, etc.).
pub const SKIP_DIRS: &[&str] = &[
    "obj", "bin", "target", "node_modules", ".git", "dist", "build",
    "__pycache__", ".venv", ".ruff_cache", ".pytest_cache", ".vs",
    ".mypy_cache", ".tox", "venv",
];

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

/// Language family for import resolution scoping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    CSharp,
    FSharp,
    Rust,
    Python,
    TypeScript,
    Go,
    Unknown,
}

impl Language {
    /// Languages that share a runtime and can import each other.
    pub fn is_compatible(self, other: Language) -> bool {
        if self == other {
            return true;
        }
        // C# and F# share the .NET runtime and can reference each other
        matches!(
            (self, other),
            (Language::CSharp, Language::FSharp) | (Language::FSharp, Language::CSharp)
        )
    }
}

/// Normalize an identifier for matching: lowercase, strip hyphens, underscores, dots.
/// Used consistently in index building AND resolution lookups.
/// e.g., "Durable-Tasks" → "durabletasks", "auto_segmentation" → "autosegmentation"
pub fn normalize_id(s: &str) -> String {
    s.to_lowercase()
        .replace('-', "")
        .replace('_', "")
}

/// Normalize for composite keys (dots preserved as separators, segments normalized).
/// e.g., "Common.Durable-Tasks" → "common.durabletasks"
pub fn normalize_dotted(s: &str) -> String {
    s.split('.')
        .map(|seg| normalize_id(seg))
        .collect::<Vec<_>>()
        .join(".")
}

/// Detect language from a file path based on extension.
pub fn detect_language(file: &str) -> Language {
    let normalized = file.replace('\\', "/");
    let file_name = normalized.rsplit('/').next().unwrap_or(&normalized);
    let ext = file_name.rsplit('.').next().unwrap_or("");
    match ext {
        "cs" | "csproj" => Language::CSharp,
        "fs" | "fsx" | "fsproj" => Language::FSharp,
        "rs" => Language::Rust,
        "py" => Language::Python,
        "ts" | "tsx" | "js" | "jsx" => Language::TypeScript,
        "go" => Language::Go,
        _ => Language::Unknown,
    }
}
