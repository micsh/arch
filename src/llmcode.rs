use crate::schema::SKIP_DIRS;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// A cross-file coupling contract parsed from a `CNTR:` line.
///
/// Syntax: `CNTR: A::symbol ↔ B::symbol: <constraint>`
#[derive(Debug, Clone)]
pub struct CntrEntry {
    /// Left side of the coupling (e.g. "stale::run").
    pub left_module: String,
    /// Right side of the coupling (e.g. "validate::check").
    pub right_module: String,
    /// Short description of the constraint.
    pub constraint: String,
}

/// A single FILE block parsed from a .llmcode file.
#[derive(Debug, Clone)]
pub struct LlmcodeBlock {
    /// Source file path (relative to project root), from the FILE: line.
    pub file: String,
    /// Arch module ID from the MOD: line, e.g. "commands/validate".
    /// None if the block has no MOD: field.
    pub mod_id: Option<String>,

    // ── Grammar fields (all optional; omit when absent in source) ──────────
    /// ROLE: — compact role/owns description.
    pub role: Option<String>,
    /// EP: — entry-point function/method names.
    pub ep: Vec<String>,
    /// INV: — invariant statements (multi-line bullet list).
    pub inv: Vec<String>,
    /// LOOK: — "also check" file paths.
    pub look: Vec<String>,
    /// EXT: — extension points.
    pub ext: Vec<String>,
    /// CHK: — cross-check file paths.
    pub chk: Vec<String>,
    /// STAB: — stability rating (stable | volatile | hotspot).
    pub stab: Option<String>,
    /// UW: — update-when triggers.
    pub uw: Vec<String>,
    /// CNTR: — cross-file coupling contracts (may appear multiple times).
    pub cntr: Vec<CntrEntry>,
}

/// Parsed representation of one .llmcode file.
#[derive(Debug)]
pub struct LlmcodeFile {
    /// Absolute path to the .llmcode file.
    pub path: PathBuf,
    /// All FILE blocks found in the file.
    pub blocks: Vec<LlmcodeBlock>,
}

/// A MOD: link that could not be resolved to a known arch module.
#[derive(Debug)]
pub struct LlmcodeValidationError {
    /// Path to the .llmcode file containing the bad reference.
    pub llmcode_path: PathBuf,
    /// The FILE: name where the MOD: field appeared.
    pub file_name: String,
    /// The unresolved MOD: value.
    pub mod_id: String,
}

impl std::fmt::Display for LlmcodeValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} ({}): MOD: '{}' does not match any declared arch module",
            self.llmcode_path.display(),
            self.file_name,
            self.mod_id,
        )
    }
}

impl LlmcodeBlock {
    fn new(file: String) -> Self {
        LlmcodeBlock {
            file,
            mod_id: None,
            role: None,
            ep: Vec::new(),
            inv: Vec::new(),
            look: Vec::new(),
            ext: Vec::new(),
            chk: Vec::new(),
            stab: None,
            uw: Vec::new(),
            cntr: Vec::new(),
        }
    }
}

/// Walk the project tree and collect all *.llmcode / *.llm files, skipping SKIP_DIRS.
///
/// Canonical location: `architecture/llmcode/` — searched recursively so both the
/// flat layout (`architecture/llmcode/commands.llmcode`) and the per-module
/// subdirectory layout (`architecture/llmcode/{container}/mod.llm`) are covered.
///
/// Falls back to a full tree walk for projects that haven't migrated yet.
pub fn discover_llmcode_files(root: &Path) -> Vec<PathBuf> {
    let canonical = root.join("architecture").join("llmcode");
    if canonical.is_dir() {
        // Canonical location — walk recursively to support subdirectory layout (P8).
        let skip: HashSet<&str> = SKIP_DIRS.iter().copied().collect();
        let mut results = Vec::new();
        for entry in WalkDir::new(&canonical)
            .into_iter()
            .filter_entry(|e| {
                !e.file_type().is_dir()
                    || !skip.contains(e.file_name().to_str().unwrap_or(""))
            })
            .flatten()
        {
            if entry.file_type().is_file() {
                let name = entry.file_name().to_string_lossy();
                if name.ends_with(".llmcode") || name.ends_with(".llm") {
                    results.push(entry.into_path());
                }
            }
        }
        results.sort(); // Deterministic order
        return results;
    }

    // Fallback: full tree walk (pre-canonical-location compatibility)
    let skip: HashSet<&str> = SKIP_DIRS.iter().copied().collect();
    let mut results = Vec::new();

    for entry in WalkDir::new(root).into_iter().filter_entry(|e| {
        if e.file_type().is_dir() {
            let name = e.file_name().to_string_lossy();
            !skip.contains(name.as_ref())
        } else {
            true
        }
    }) {
        let Ok(entry) = entry else { continue };
        if entry.file_type().is_file() {
            let name = entry.file_name().to_string_lossy();
            if name.ends_with(".llmcode") || name.ends_with(".llm") {
                results.push(entry.into_path());
            }
        }
    }

    results
}

/// Parse a .llmcode file from its raw text content.
///
/// Extracts FILE blocks with all recognized grammar fields (MOD:, ROLE:, EP:,
/// INV:, LOOK:, EXT:, CHK:, STAB:, UW:, CNTR:). Unrecognized lines are skipped.
/// Multi-value fields (EP:, LOOK:, EXT:, CHK:, UW:) accept semicolon-separated values.
/// INV: and CNTR: are multi-line bullet lists (lines starting with `- `).
/// ASSUMPTION: the parser is lenient — unknown keys and free text are silently skipped.
/// IF INVALID: add a strict mode that errors on unrecognized fields.
pub fn parse_llmcode_file(path: &Path, content: &str) -> Result<LlmcodeFile, String> {
    let mut blocks: Vec<LlmcodeBlock> = Vec::new();
    let mut current: Option<LlmcodeBlock> = None;
    // Track which multi-line section we're inside (INV or CNTR bullet list)
    let mut in_inv = false;
    let mut in_cntr = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // FILE: starts a new block — commit previous
        if let Some(rest) = trimmed.strip_prefix("FILE:") {
            if let Some(block) = current.take() {
                blocks.push(block);
            }
            current = Some(LlmcodeBlock::new(rest.trim().to_string()));
            in_inv = false;
            in_cntr = false;
            continue;
        }

        let Some(ref mut block) = current else { continue };

        // Blank line resets multi-line section tracking
        if trimmed.is_empty() {
            in_inv = false;
            in_cntr = false;
            continue;
        }

        // Single-value fields
        if let Some(rest) = trimmed.strip_prefix("MOD:") {
            block.mod_id = Some(rest.trim().to_string());
            in_inv = false; in_cntr = false;
        } else if let Some(rest) = trimmed.strip_prefix("ROLE:") {
            block.role = Some(rest.trim().to_string());
            in_inv = false; in_cntr = false;
        } else if let Some(rest) = trimmed.strip_prefix("STAB:") {
            block.stab = Some(rest.trim().to_string());
            in_inv = false; in_cntr = false;
        // Semi-colon separated fields
        } else if let Some(rest) = trimmed.strip_prefix("EP:") {
            block.ep = split_semi(rest);
            in_inv = false; in_cntr = false;
        } else if let Some(rest) = trimmed.strip_prefix("LOOK:") {
            block.look = split_semi(rest);
            in_inv = false; in_cntr = false;
        } else if let Some(rest) = trimmed.strip_prefix("EXT:") {
            block.ext = split_semi(rest);
            in_inv = false; in_cntr = false;
        } else if let Some(rest) = trimmed.strip_prefix("CHK:") {
            block.chk = split_semi(rest);
            in_inv = false; in_cntr = false;
        } else if let Some(rest) = trimmed.strip_prefix("UW:") {
            block.uw = split_semi(rest);
            in_inv = false; in_cntr = false;
        // Multi-line sections
        } else if trimmed == "INV:" {
            in_inv = true; in_cntr = false;
        } else if trimmed.starts_with("CNTR:") {
            in_cntr = true; in_inv = false;
            // Inline CNTR: value (single line, not a section header)
            let rest = trimmed.strip_prefix("CNTR:").unwrap_or("").trim();
            if !rest.is_empty() {
                if let Some(entry) = parse_cntr_entry(rest) {
                    block.cntr.push(entry);
                }
            }
        } else if in_inv && trimmed.starts_with("- ") {
            block.inv.push(trimmed[2..].to_string());
        } else if in_cntr && trimmed.starts_with("- ") {
            if let Some(entry) = parse_cntr_entry(&trimmed[2..]) {
                block.cntr.push(entry);
            }
        }
    }

    // Commit final block
    if let Some(block) = current {
        blocks.push(block);
    }

    Ok(LlmcodeFile {
        path: path.to_path_buf(),
        blocks,
    })
}

/// Split a semicolon-separated field value into trimmed tokens.
fn split_semi(s: &str) -> Vec<String> {
    s.split(';').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect()
}

/// Parse a CNTR: entry from its content (after the `CNTR:` prefix or `- ` bullet).
///
/// Expected format: `A::symbol ↔ B::symbol: <constraint>`
/// Returns None if the format is not recognizable.
fn parse_cntr_entry(s: &str) -> Option<CntrEntry> {
    // Split on the arrow ↔ (U+2194) to separate left and right
    let arrow = '\u{2194}';
    let parts: Vec<&str> = s.splitn(2, arrow).collect();
    if parts.len() != 2 {
        return None;
    }
    let left = parts[0].trim().to_string();
    // Right side: "B::symbol: <constraint>" — use ": " to avoid splitting on "::" 
    let right_part = parts[1].trim();
    let (right, constraint) = if let Some(colon_pos) = right_part.find(": ") {
        (
            right_part[..colon_pos].trim().to_string(),
            right_part[colon_pos + 2..].trim().to_string(),
        )
    } else if let Some(colon_pos) = right_part.rfind(':') {
        // Fallback: last colon (handles no-space case)
        (
            right_part[..colon_pos].trim().to_string(),
            right_part[colon_pos + 1..].trim().to_string(),
        )
    } else {
        (right_part.to_string(), String::new())
    };
    Some(CntrEntry {
        left_module: left,
        right_module: right,
        constraint,
    })
}

/// Validate MOD: links against the set of declared arch module IDs.
///
/// `valid_ids` is the exhaustive set of "container/module" IDs from the
/// architecture .arch files — the caller builds it from ArchContext.
///
/// Blocks without a MOD: field produce a warning, not an error — this cycle
/// allows partial coverage; future cycles will tighten to hard error.
/// ASSUMPTION: cross-project MOD: references (containing '//' or project prefix)
/// are accepted syntactically and skipped for validation. IF INVALID: add
/// cross-project ID expansion before calling contains().
pub fn validate_mod_links(
    files: &[LlmcodeFile],
    valid_ids: &HashSet<String>,
) -> Vec<LlmcodeValidationError> {
    let mut errors = Vec::new();

    for llmcode_file in files {
        for block in &llmcode_file.blocks {
            let Some(ref mod_id) = block.mod_id else {
                // No MOD: field — warning-only this cycle
                continue;
            };

            // ASSUMPTION: cross-project references contain '/' in addition to the local
            // container/module separator. Simple heuristic: more than one '/' means
            // cross-project. IF INVALID: refine the cross-project detection.
            let slash_count = mod_id.chars().filter(|&c| c == '/').count();
            if slash_count > 1 {
                // Cross-project reference — skip local validation (Phase A per ADR)
                continue;
            }

            if !valid_ids.contains(mod_id.as_str()) {
                errors.push(LlmcodeValidationError {
                    llmcode_path: llmcode_file.path.clone(),
                    file_name: block.file.clone(),
                    mod_id: mod_id.clone(),
                });
            }
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn make_valid_ids(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_extracts_mod_from_single_block() {
        let content = "AREA: commands\n\nFILE: schema.rs\nMOD: schema/schema\n";
        let result = parse_llmcode_file(Path::new("test.llmcode"), content).unwrap();
        assert_eq!(result.blocks.len(), 1);
        assert_eq!(result.blocks[0].file, "schema.rs");
        assert_eq!(result.blocks[0].mod_id.as_deref(), Some("schema/schema"));
    }

    #[test]
    fn parse_extracts_multiple_blocks() {
        let content = "\
FILE: validate.rs
MOD: commands/validate

FILE: coverage.rs
MOD: commands/coverage
";
        let result = parse_llmcode_file(Path::new("test.llmcode"), content).unwrap();
        assert_eq!(result.blocks.len(), 2);
        assert_eq!(result.blocks[0].mod_id.as_deref(), Some("commands/validate"));
        assert_eq!(result.blocks[1].mod_id.as_deref(), Some("commands/coverage"));
    }

    #[test]
    fn parse_block_without_mod_is_none() {
        let content = "FILE: schema.rs\nOWNS: domain types\n";
        let result = parse_llmcode_file(Path::new("test.llmcode"), content).unwrap();
        assert_eq!(result.blocks.len(), 1);
        assert!(result.blocks[0].mod_id.is_none());
    }

    #[test]
    fn validate_returns_error_for_unknown_mod() {
        let mut block = LlmcodeBlock::new("schema.rs".to_string());
        block.mod_id = Some("nonexistent/module".to_string());
        let file = LlmcodeFile { path: PathBuf::from("test.llmcode"), blocks: vec![block] };
        let valid = make_valid_ids(&["schema/schema"]);
        let errors = validate_mod_links(&[file], &valid);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].mod_id, "nonexistent/module");
    }

    #[test]
    fn validate_passes_for_known_mod() {
        let mut block = LlmcodeBlock::new("schema.rs".to_string());
        block.mod_id = Some("schema/schema".to_string());
        let file = LlmcodeFile { path: PathBuf::from("test.llmcode"), blocks: vec![block] };
        let valid = make_valid_ids(&["schema/schema"]);
        let errors = validate_mod_links(&[file], &valid);
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_skips_blocks_without_mod() {
        let block = LlmcodeBlock::new("schema.rs".to_string());
        let file = LlmcodeFile { path: PathBuf::from("test.llmcode"), blocks: vec![block] };
        let valid = make_valid_ids(&[]);
        let errors = validate_mod_links(&[file], &valid);
        assert!(errors.is_empty());
    }

    #[test]
    fn parse_extracts_grammar_fields() {
        let content = "\
FILE: schema.rs
MOD: schema/schema
ROLE: domain types and constants
EP: Architecture; Container; Module
STAB: stable
INV:
- SKIP_DIRS must never include src — it is a source root
CNTR: stale::run ↔ validate::check: ValidationResult fields must stay in sync
";
        let result = parse_llmcode_file(Path::new("test.llmcode"), content).unwrap();
        assert_eq!(result.blocks.len(), 1);
        let b = &result.blocks[0];
        assert_eq!(b.role.as_deref(), Some("domain types and constants"));
        assert_eq!(b.ep, vec!["Architecture", "Container", "Module"]);
        assert_eq!(b.stab.as_deref(), Some("stable"));
        assert_eq!(b.inv, vec!["SKIP_DIRS must never include src — it is a source root"]);
        assert_eq!(b.cntr.len(), 1);
        assert_eq!(b.cntr[0].left_module, "stale::run");
        assert_eq!(b.cntr[0].right_module, "validate::check");
        assert!(b.cntr[0].constraint.contains("ValidationResult"));
    }

    #[test]
    fn parse_cntr_entry_parses_correctly() {
        let entry = parse_cntr_entry("stale::run \u{2194} validate::check: must stay in sync").unwrap();
        assert_eq!(entry.left_module, "stale::run");
        assert_eq!(entry.right_module, "validate::check");
        assert_eq!(entry.constraint, "must stay in sync");
    }
}
