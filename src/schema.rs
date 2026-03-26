use glob::Pattern;
use serde::Serialize;

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

/// Root architecture structure — loaded from .arch source files.
#[derive(Debug, Serialize)]
pub struct Architecture {
    #[serde(default)]
    pub guidance: Option<String>,
    pub system: System,
    pub containers: Vec<Container>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Serialize)]
pub struct System {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub ignore: Vec<String>,
}

#[derive(Debug, Serialize)]
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

/// Per-container module definitions — loaded from architecture/arch/containers/{id}.arch
#[derive(Debug, Serialize)]
pub struct ContainerDetail {
    #[serde(default)]
    pub modules: Vec<Module>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
pub struct Rule {
    pub id: String,
    #[serde(rename = "type")]
    pub rule_type: String,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<Vec<String>>,
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
    /// Exhaustive list of modules permitted to depend on `module` (used by `restrict_callers_to`).
    ///
    /// USAGE NOTE: For sub-module precision, isolate the target module into its own container.
    /// The rule enforces the boundary; the container makes the boundary resolvable at import level.
    /// Example: to restrict callers of Parser.fs specifically, give it its own container
    /// (`protocols-parser`) rather than targeting `protocols/parser` within a shared container.
    #[serde(default)]
    pub allowed: Vec<String>,
    /// Advisory threshold: if `allowed.len() > allowed_max`, emit an informational advisory.
    /// Does not flip passed/failed — advisory is exit 0.
    #[serde(default)]
    pub allowed_max: Option<usize>,
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

/// Full llmcode grammar specification text.
pub const LLMCODE_SPEC: &str = r#"llmcode grammar spec v0.4
=======================================================

OVERVIEW
  .llm files document modules for AI-assisted coding.
  One FILE block per module, stored in architecture/llmcode/{container}/{module}.llm

FILE BLOCK STRUCTURE
  FILE: <source-file-path>        — starts a block; path relative to container
  MOD: <container/module>         — owning module ID (required; used for link validation)
  ROLE: <text>                    — one-line role description
  EP: <fn>; <fn>                  — public entry points (semicolon-separated)
  STAB: stable|volatile|hotspot   — change frequency
  INV:                            — multi-line key invariants (bullet list)
    - <invariant text>
    - <invariant text>
  LOOK: <thing>; <thing>          — things to look up before editing
  EXT: <thing>; <thing>           — external dependencies / assumptions
  CHK: <thing>; <thing>           — things to check after editing
  UW: <thing>; <thing>            — unsafe/unwrap markers (explain why safe)
  CNTR: <mod::symbol> <op> <mod::symbol>  — contracts between modules
  DEPS-ON: <mod>; <mod>           — declared dependencies (informational)

CNTR CONTRACT FORMAT
  CNTR: left_module::symbol <constraint> right_module::symbol
  Constraints: depends_on | used_by | must_not | compatible_with

PARSING RULES
  - FILE: starts a new block; commits the previous block
  - Blank lines reset INV:/CNTR: multi-line tracking
  - MOD: uses container/module format for cross-reference validation
  - Lines starting with '#' are comments
  - Unknown fields are silently skipped (forward compatible)
  - STAB values: stable (rarely changes), volatile (changes often), hotspot (contention risk)

EXAMPLES
  FILE: context.rs
  MOD: context/context
  ROLE: shared architecture loader — ArchContext, file walking, JSON output
  EP: ArchContext::load; walk_module_files; print_json
  STAB: stable
  INV:
    - context must never import from commands or scanner
    - walk_module_files callers must pass collect_mapped_files output, not empty set
  DEPS-ON: schema; resolve; arch_parser
"#;

/// Full .arch grammar specification text.
pub const ARCH_SPEC: &str = r#".arch grammar spec v0.4
=======================================================

OVERVIEW
  Two file types define a project's architecture:
    architecture/arch/system.arch        — system declaration, rules, stories
    architecture/arch/containers/{id}.arch — one per container, modules defined here

=======================================================
SYSTEM.ARCH FIELDS
=======================================================

  ARCH: 0.4                 Required. Grammar version. First line of file.
  SYS: <name>               Required. System name. No slashes or spaces.
  DESC: <text>              Required. One-line system description.
  IGNORE: <glob>; <glob>    Optional. Semicolon-separated glob patterns — paths excluded from
                              coverage, drift, and fitness scans.
  GUIDE:                    Optional. Multi-line guidance block for AI/developers.
    <line>                    All lines after GUIDE: until ENDGUIDE are accumulated.
    <line>
  ENDGUIDE                  Terminates the GUIDE: block.
  CONT: <id>                Required (one+). Container ID declaration. One per line.
                              Compiler cross-checks each ID has a matching containers/{id}.arch.

RULE BLOCKS (in system.arch)
  RULE: <id>                Starts a rule block. Must be unique.
  TYPE: <type>              Required. One of: no_dependency | no_import_from |
                              restrict_callers_to | boundary
  FROM: <target>            Required. container ID or container/module ID.
  TO: <id>; <id>            Required for no_dependency, no_import_from.
                              Semicolon-separated container or container/module IDs.
  MODULE: <id>              Required for restrict_callers_to. The protected module.
  ALLOWED: <id>; <id>       Required for restrict_callers_to.
                              Semicolon-separated allowed caller module IDs (container/module form).
  CONSTRAINT: <text>        Required for boundary. Free-text constraint statement.
  WHY: <text>               Optional. Human-readable reason for the rule.

STORY BLOCKS (in system.arch)
  STORY: <id>               Starts a story block.
  DESC: <text>              Required. One-line story description.
  FLOW: <id>; <id>          Required. Semicolon-separated module IDs in flow order.
                              Use container/module form.

=======================================================
CONTAINER .ARCH FIELDS
=======================================================

  ARCH: 0.4                 Required. Grammar version. First line.
  CONT: <id>                Required. Container ID (must match system.arch CONT: declaration).
  PROJ: <name>              Optional. Project name for qualified IDs. Defaults to SYS:.
  PATH: <path>              Required. Source path relative to project root.
  DEP: <id>; <id>           Optional. Semicolon-separated container IDs this container depends on.
  DESC: <text>              Optional. One-line container description.

MODULE BLOCKS (in container .arch)
  MOD: <id>                 Starts a module block. ID is the short form (no container prefix).
  FILE: <path>              Use FILE: or FILES:, not both. Path relative to container PATH.
  FILES: <path>; <path>     Multi-file module. Semicolon-separated paths relative to PATH.
  OWN: <concept>; <concept> Required. Semicolon-separated owned concepts.
                            Note: OWN: entries are matched against actual import strings by the
                            resolver — include namespace prefixes (e.g. `Conductor.Core.Types`)
                            alongside concept labels. Using concept labels only may cause
                            broad-match fallback and phantom fitness violations.
  BND: <text>               Optional. Boundary — what this module does NOT do.
  DEP: <id>; <id>           Optional. Semicolon-separated depends_on in container/module form.
  ROUTES: <name>; <name>    Optional. Public entry points / route names.

=======================================================
PARSING RULES
=======================================================

  - Lines beginning with '#' are comments — ignored.
  - Blank lines are ignored.
  - Keywords match on "KEYWORD:" prefix (colon required) — ordering-independent.
  - Block keywords (CONT:, MOD:, RULE:, STORY:) commit the current block and start a new one.
  - GUIDE: is multi-line: accumulates lines until ENDGUIDE or next keyword.
  - List fields (TO:, ALLOWED:, DEP:, OWN:, FILES:, FLOW:, ROUTES:) split on ';'.
  - Empty elements after trim are dropped.
  - FILE: and FILES: are mutually exclusive per MOD block — parser error if both present.
  - PROJ: defaults to SYS: value from system.arch (passed as context during container parse).
  - RULE:/STORY: in container files are warnings and skipped.
  - MOD: in system.arch is a warning and skipped.
  - Unknown keywords are silently skipped (forward compatible).

=======================================================
EXAMPLES
=======================================================

system.arch:
  ARCH: 0.4
  SYS: myapp
  DESC: My application
  GUIDE:
    Read the relevant container .arch before editing.
    Update OWN: and DEP: if your change crosses modules.
  ENDGUIDE

  CONT: api
  CONT: core
  CONT: db

  RULE: core-independence
    TYPE: no_dependency
    FROM: core
    TO: api; db
    WHY: Core must not depend on presentation or storage layers

  STORY: user-login
    DESC: User submits credentials and receives a token
    FLOW: api/gateway; core/auth; db/users

containers/api.arch:
  ARCH: 0.4
  CONT: api
  PATH: src/api
  DEP: core
  DESC: HTTP layer — routes, handlers, middleware

  MOD: gateway
    FILE: gateway.rs
    OWN: request-routing; middleware-chain
    BND: HTTP boundary only — no business logic
    DEP: core/auth
"#;

/// Parsed output of a complete `.arch` source tree.
///
/// Produced by `arch_parser::parse_system_arch` + `arch_parser::parse_container_arch`.
/// Consumed by `context.rs` to build ArchContext from `.arch` source files.
///
/// DR-03: This struct is the explicit contract between arch_parser (pure leaf) and
/// context.rs (load() internals). No other command may depend on arch_parser — see
/// the `arch-parser-callers` fitness rule.
#[derive(Debug, Default)]
pub struct ArchSource {
    /// Grammar version from ARCH: line.
    pub arch_version: String,
    /// System name from SYS:.
    pub system_name: String,
    /// System description from DESC:.
    pub system_description: String,
    /// Multi-line guidance block from GUIDE:.
    pub guidance: String,
    /// Glob patterns from IGNORE: lines — paths to exclude from coverage/drift/fitness.
    pub ignore: Vec<String>,
    /// Ordered container ID list from CONT: lines.
    pub container_ids: Vec<String>,
    /// Fully parsed containers (populated after per-container parse).
    pub containers: Vec<ArchSourceContainer>,
    /// Rule blocks from RULE: sections.
    pub rules: Vec<ArchSourceRule>,
    /// Story blocks from STORY: sections.
    pub stories: Vec<ArchSourceStory>,
}

/// One container declaration parsed from a container `.arch` file.
#[derive(Debug, Default)]
pub struct ArchSourceContainer {
    pub id: String,
    pub project: Option<String>,
    pub path: String,
    pub description: Option<String>,
    /// Semicolon-separated depends_on parsed to Vec.
    pub depends_on: Vec<String>,
    pub modules: Vec<ArchSourceModule>,
}

/// One module block inside a container `.arch` file.
#[derive(Debug, Default)]
pub struct ArchSourceModule {
    pub id: String,
    /// Single source file (FILE:). Mutually exclusive with `files`.
    pub file: Option<String>,
    /// Multiple source files (FILES:). Mutually exclusive with `file`.
    pub files: Vec<String>,
    pub owns: Vec<String>,
    pub boundary: Option<String>,
    pub depends_on: Vec<String>,
    pub routes: Vec<String>,
}

/// One RULE: block from system.arch.
#[derive(Debug, Default)]
pub struct ArchSourceRule {
    pub id: String,
    pub rule_type: String,
    pub from: Option<String>,
    pub to: Vec<String>,
    pub module: Option<String>,
    pub allowed: Vec<String>,
    pub allowed_max: Option<usize>,
    pub constraint: Option<String>,
    pub reason: Option<String>,
}

/// One STORY: block from system.arch.
#[derive(Debug, Default)]
pub struct ArchSourceStory {
    pub id: String,
    pub description: String,
    pub flow: Vec<String>,
}

/// Stories parsed from STORY: blocks in system.arch
#[derive(Debug, Clone, Serialize)]
pub struct Stories {
    #[serde(default)]
    pub stories: Vec<Story>,
}

#[derive(Debug, Clone, Serialize)]
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
