use glob::Pattern;
use serde::{Deserialize, Serialize};

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

/// Normalize an identifier for matching: lowercase, strip hyphens, underscores.
/// Used consistently in index building AND resolution lookups.
/// Returns None for empty/whitespace-only input or input that normalizes to empty
/// (e.g., "---", "___").
/// e.g., "Durable-Tasks" → Some("durabletasks"), "auto_segmentation" → Some("autosegmentation")
pub fn normalize_id(s: &str) -> Option<String> {
    let result = s.to_lowercase().replace('-', "").replace('_', "");
    if result.is_empty() { None } else { Some(result) }
}

/// Normalize for composite keys (dots preserved as separators, segments normalized).
/// Drops empty segments (e.g., "a...b" → "a.b", not "a...b").
/// e.g., "Common.Durable-Tasks" → "common.durabletasks"
pub fn normalize_dotted(s: &str) -> String {
    s.split('.')
        .filter_map(|seg| normalize_id(seg))
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

/// A `pub use` re-export statement parsed from a Rust file.
#[derive(Debug, Clone)]
pub struct PubUseEntry {
    /// The local submodule being re-exported from (e.g. "types" from `pub use types::*`).
    /// Only bare local names are captured — `crate::` and `super::` forms are skipped.
    pub source_module: String,
    /// None = wildcard (`*`), Some(name) = specific named export.
    pub symbol: Option<String>,
}

/// Parse `pub use` re-export statements from Rust source text.
///
/// Returns one entry per exported name. Handles `::*`, `::Name`, and `::{A, B}` forms.
/// Strips `self::` prefix automatically.
///
/// # Skipped forms
/// // ASSUMPTION: `pub use crate::X` and `pub use super::X` re-exports are not followed.
/// // IF INVALID (a module re-exports from a non-local path): the re-export collapse in
/// // resolve.rs will simply not apply — safe fallback, import stays as [broad match].
pub fn extract_rust_pub_uses(content: &str) -> Vec<PubUseEntry> {
    let mut entries = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("pub use ") || !trimmed.ends_with(';') {
            continue;
        }

        let rest = trimmed
            .strip_prefix("pub use ")
            .unwrap()
            .trim_end_matches(';')
            .trim();

        // Strip self:: prefix (pub use self::types::* → types::*)
        let rest = rest.strip_prefix("self::").unwrap_or(rest);

        // Skip cross-scope re-exports — not traceable without full module graph resolution
        if rest.starts_with("crate::") || rest.starts_with("super::") {
            continue;
        }

        if let Some(brace_pos) = rest.find("::{") {
            // module::{A, B, C} form
            let prefix = &rest[..brace_pos];
            let source_module = prefix.split("::").last().unwrap_or(prefix).to_string();
            if source_module.is_empty() {
                continue;
            }
            let inner = rest[brace_pos + 3..].trim_end_matches('}');
            for item in inner.split(',') {
                let item = item.trim();
                if item.is_empty() || item == ".." {
                    continue;
                }
                entries.push(PubUseEntry { source_module: source_module.clone(), symbol: Some(item.to_string()) });
            }
        } else if rest.ends_with("::*") {
            // module::* wildcard
            let prefix = rest.strip_suffix("::*").unwrap();
            let source_module = prefix.split("::").last().unwrap_or(prefix).to_string();
            if source_module.is_empty() {
                continue;
            }
            entries.push(PubUseEntry { source_module, symbol: None });
        } else if let Some(last_sep) = rest.rfind("::") {
            // module::Name (or a::b::Name — take only the immediate parent segment)
            let prefix = &rest[..last_sep];
            let symbol = &rest[last_sep + 2..];
            let source_module = prefix.split("::").last().unwrap_or(prefix).to_string();
            if source_module.is_empty() || symbol.is_empty() {
                continue;
            }
            entries.push(PubUseEntry { source_module, symbol: Some(symbol.to_string()) });
        }
        // Single bare name (no ::) — skip; ambiguous (could be `use types;` module alias)
    }

    entries
}

#[cfg(test)]
mod schema_tests {
    use super::*;

    #[test]
    fn test_pub_use_wildcard() {
        let content = "pub use types::*;\n";
        let entries = extract_rust_pub_uses(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source_module, "types");
        assert!(entries[0].symbol.is_none());
    }

    #[test]
    fn test_pub_use_named() {
        let content = "pub use types::PresenceStatus;\n";
        let entries = extract_rust_pub_uses(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source_module, "types");
        assert_eq!(entries[0].symbol.as_deref(), Some("PresenceStatus"));
    }

    #[test]
    fn test_pub_use_self_prefix_stripped() {
        let content = "pub use self::types::*;\n";
        let entries = extract_rust_pub_uses(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source_module, "types");
        assert!(entries[0].symbol.is_none());
    }

    #[test]
    fn test_pub_use_brace_group() {
        let content = "pub use types::{A, B};\n";
        let entries = extract_rust_pub_uses(content);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].source_module, "types");
        assert_eq!(entries[0].symbol.as_deref(), Some("A"));
        assert_eq!(entries[1].source_module, "types");
        assert_eq!(entries[1].symbol.as_deref(), Some("B"));
    }

    #[test]
    fn test_pub_use_crate_skipped() {
        let content = "pub use crate::other::Foo;\n";
        let entries = extract_rust_pub_uses(content);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_pub_use_super_skipped() {
        let content = "pub use super::parent::Bar;\n";
        let entries = extract_rust_pub_uses(content);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_pub_use_bare_name_skipped() {
        let content = "pub use types;\n";
        let entries = extract_rust_pub_uses(content);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_non_pub_use_ignored() {
        let content = "use types::*;\nmod types;\n";
        let entries = extract_rust_pub_uses(content);
        assert!(entries.is_empty());
    }
}
