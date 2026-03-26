// arch_parser — .arch source file discovery and parsing.
//
// Pure leaf module: depends only on schema.rs types.
// Never calls ArchContext::load(). Never writes files.

use crate::schema::{
    ArchSource, ArchSourceContainer, ArchSourceModule, ArchSourceRule, ArchSourceStory,
};
use std::path::{Path, PathBuf};

// ────────────────────────────────────────────────────────────────────────────
// Discovery
// ────────────────────────────────────────────────────────────────────────────

/// Return all `.arch` files under `root/architecture/arch/` in sorted order.
///
/// Expects:
///   architecture/arch/system.arch         — system declaration
///   architecture/arch/containers/{id}.arch — one per container
///
/// ASSUMPTION: `.arch` files live exclusively in `architecture/arch/` and its
/// immediate `containers/` subdirectory. IF INVALID: extend to a recursive walk.
pub fn discover_arch_files(root: &Path) -> Vec<PathBuf> {
    let arch_dir = root.join("architecture").join("arch");
    if !arch_dir.is_dir() {
        return Vec::new();
    }

    let mut results = Vec::new();

    // Top-level .arch files (system.arch)
    if let Ok(entries) = std::fs::read_dir(&arch_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("arch") {
                results.push(path);
            }
        }
    }

    // containers/ subdirectory
    let containers_dir = arch_dir.join("containers");
    if let Ok(entries) = std::fs::read_dir(&containers_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("arch") {
                results.push(path);
            }
        }
    }

    results.sort();
    results
}

// ────────────────────────────────────────────────────────────────────────────
// System.arch parser
// ────────────────────────────────────────────────────────────────────────────

/// Parse `system.arch` content into an `ArchSource`.
///
/// Populates `arch_version`, `system_name`, `system_description`, `guidance`,
/// `container_ids`, `rules`, and `stories`. Container details are filled later
/// by calling `parse_container_arch` for each declared container.
///
/// Parsing invariants (from grammar spec a-aee91ac0):
/// 1. Block keywords followed by `: ` or `:\n` start a new field.
/// 2. GUIDE: accumulates all subsequent non-keyword lines until the next keyword.
/// 3. RULE:/STORY: commit the current block and start a new one.
/// 4. MOD: in system.arch is invalid — emits a warning and is skipped.
/// 5. Unknown keywords are silently skipped for forward compatibility.
pub fn parse_system_arch(content: &str) -> Result<ArchSource, String> {
    let mut src = ArchSource::default();

    #[derive(PartialEq)]
    enum Section { None, Guide, Rule, Story }

    let mut section = Section::None;
    let mut current_rule: Option<ArchSourceRule> = None;
    let mut current_story: Option<ArchSourceStory> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        // Comments and blank lines
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Keyword dispatch
        if let Some(rest) = keyword_value(trimmed, "ARCH") {
            flush_rule(&mut current_rule, &mut src.rules);
            flush_story(&mut current_story, &mut src.stories);
            section = Section::None;
            src.arch_version = rest.to_string();
        } else if let Some(rest) = keyword_value(trimmed, "SYS") {
            flush_rule(&mut current_rule, &mut src.rules);
            flush_story(&mut current_story, &mut src.stories);
            section = Section::None;
            src.system_name = rest.to_string();
        } else if let Some(rest) = keyword_value(trimmed, "DESC") {
            // DESC at top level = system description; inside STORY = story description
            if section == Section::Story {
                if let Some(ref mut s) = current_story {
                    s.description = rest.to_string();
                }
            } else {
                flush_rule(&mut current_rule, &mut src.rules);
                flush_story(&mut current_story, &mut src.stories);
                section = Section::None;
                src.system_description = rest.to_string();
            }
        } else if keyword_value(trimmed, "GUIDE").is_some() {
            flush_rule(&mut current_rule, &mut src.rules);
            flush_story(&mut current_story, &mut src.stories);
            // Value after GUIDE: (if any) starts the block
            let after = keyword_value(trimmed, "GUIDE").unwrap_or_default();
            if !after.is_empty() {
                src.guidance = after.to_string();
            }
            section = Section::Guide;
        } else if trimmed == "ENDGUIDE" {
            // Terminate the GUIDE: multi-line block
            section = Section::None;
        } else if let Some(rest) = keyword_value(trimmed, "IGNORE") {
            flush_rule(&mut current_rule, &mut src.rules);
            flush_story(&mut current_story, &mut src.stories);
            section = Section::None;
            src.ignore.extend(split_semi(rest));
        } else if let Some(rest) = keyword_value(trimmed, "CONT") {
            flush_rule(&mut current_rule, &mut src.rules);
            flush_story(&mut current_story, &mut src.stories);
            section = Section::None;
            let id = rest.trim().to_string();
            if !id.is_empty() {
                src.container_ids.push(id);
            }
        } else if let Some(rest) = keyword_value(trimmed, "RULE") {
            flush_rule(&mut current_rule, &mut src.rules);
            flush_story(&mut current_story, &mut src.stories);
            section = Section::Rule;
            current_rule = Some(ArchSourceRule { id: rest.to_string(), ..Default::default() });
        } else if let Some(rest) = keyword_value(trimmed, "STORY") {
            flush_rule(&mut current_rule, &mut src.rules);
            flush_story(&mut current_story, &mut src.stories);
            section = Section::Story;
            current_story = Some(ArchSourceStory { id: rest.to_string(), ..Default::default() });
        } else if let Some(rest) = keyword_value(trimmed, "TYPE") {
            if let Some(ref mut r) = current_rule {
                r.rule_type = rest.to_string();
            }
        } else if let Some(rest) = keyword_value(trimmed, "FROM") {
            if let Some(ref mut r) = current_rule {
                r.from = Some(rest.to_string());
            }
        } else if let Some(rest) = keyword_value(trimmed, "TO") {
            if let Some(ref mut r) = current_rule {
                r.to = split_semi(rest);
            }
        } else if let Some(rest) = keyword_value(trimmed, "ALLOWED") {
            if let Some(ref mut r) = current_rule {
                r.allowed = split_semi(rest);
            }
        } else if let Some(rest) = keyword_value(trimmed, "ALLOWED_MAX") {
            if let Some(ref mut r) = current_rule {
                r.allowed_max = rest.trim().parse::<usize>().ok();
            }
        } else if let Some(rest) = keyword_value(trimmed, "MODULE") {
            if let Some(ref mut r) = current_rule {
                r.module = Some(rest.to_string());
            }
        } else if let Some(rest) = keyword_value(trimmed, "CONSTRAINT") {
            if let Some(ref mut r) = current_rule {
                r.constraint = Some(rest.to_string());
            }
        } else if let Some(rest) = keyword_value(trimmed, "WHY") {
            if let Some(ref mut r) = current_rule {
                r.reason = Some(rest.to_string());
            }
        } else if let Some(rest) = keyword_value(trimmed, "FLOW") {
            if let Some(ref mut s) = current_story {
                s.flow = split_semi(rest);
            }
        } else if keyword_value(trimmed, "MOD").is_some() {
            // MOD: is only valid in container files
            eprintln!("⚠️  arch_parser: MOD: found in system.arch — ignored (only valid in container files)");
        } else if section == Section::Guide {
            // Accumulate GUIDE: multi-line block
            if !src.guidance.is_empty() {
                src.guidance.push('\n');
            }
            src.guidance.push_str(trimmed);
        }
        // Unknown keywords outside GUIDE are silently skipped
    }

    // Flush final blocks
    flush_rule(&mut current_rule, &mut src.rules);
    flush_story(&mut current_story, &mut src.stories);

    Ok(src)
}

// ────────────────────────────────────────────────────────────────────────────
// Container.arch parser
// ────────────────────────────────────────────────────────────────────────────

/// Parse a `containers/{id}.arch` file into an `ArchSourceContainer`.
///
/// `sys_name` is passed from the system parse result; PROJ: defaults to it when absent.
///
/// Parsing invariants enforced:
/// 1. FILE: and FILES: are mutually exclusive per MOD block — error if both present.
/// 2. RULE:/STORY: in container files are invalid — emitted as warnings and skipped.
/// 3. Unknown keywords are silently skipped.
/// 4. List fields (OWN:, DEP:, FILES:, ROUTES:) split on ';'.
/// 5. PROJ: defaults to `sys_name` when absent.
pub fn parse_container_arch(content: &str, id: &str, sys_name: &str) -> Result<ArchSourceContainer, String> {
    let mut container = ArchSourceContainer {
        id: id.to_string(),
        project: None,
        ..Default::default()
    };

    let mut current_mod: Option<ArchSourceModule> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(rest) = keyword_value(trimmed, "ARCH") {
            // Grammar version — ignore in container file (already read from system.arch)
            let _ = rest;
        } else if let Some(rest) = keyword_value(trimmed, "CONT") {
            // Container ID declaration — id already provided by caller
            let _ = rest;
        } else if let Some(rest) = keyword_value(trimmed, "PROJ") {
            container.project = Some(rest.to_string());
        } else if let Some(rest) = keyword_value(trimmed, "PATH") {
            container.path = rest.to_string();
        } else if let Some(rest) = keyword_value(trimmed, "DEP") {
            // Container-level dependencies (when not inside a MOD block)
            if current_mod.is_none() {
                container.depends_on = split_semi(rest);
            } else if let Some(ref mut m) = current_mod {
                m.depends_on = split_semi(rest);
            }
        } else if let Some(rest) = keyword_value(trimmed, "DESC") {
            if current_mod.is_none() {
                container.description = Some(rest.to_string());
            }
            // DESC inside MOD: not part of schema — silently skip
        } else if let Some(rest) = keyword_value(trimmed, "MOD") {
            // Commit previous module
            if let Some(m) = current_mod.take() {
                validate_module_files(&m)?;
                container.modules.push(m);
            }
            current_mod = Some(ArchSourceModule {
                id: rest.to_string(),
                ..Default::default()
            });
        } else if let Some(rest) = keyword_value(trimmed, "FILE") {
            if let Some(ref mut m) = current_mod {
                m.file = Some(rest.to_string());
            }
        } else if let Some(rest) = keyword_value(trimmed, "FILES") {
            if let Some(ref mut m) = current_mod {
                m.files = split_semi(rest);
            }
        } else if let Some(rest) = keyword_value(trimmed, "OWN") {
            if let Some(ref mut m) = current_mod {
                m.owns = split_semi(rest);
            }
        } else if let Some(rest) = keyword_value(trimmed, "BND") {
            if let Some(ref mut m) = current_mod {
                m.boundary = Some(rest.to_string());
            }
        } else if let Some(rest) = keyword_value(trimmed, "ROUTES") {
            if let Some(ref mut m) = current_mod {
                m.routes = split_semi(rest);
            }
        } else if keyword_value(trimmed, "RULE").is_some()
            || keyword_value(trimmed, "STORY").is_some()
        {
            eprintln!("⚠️  arch_parser: RULE:/STORY: found in container file '{id}.arch' — ignored (only valid in system.arch)");
        }
        // Unknown keywords silently skipped
    }

    // Flush last module
    if let Some(m) = current_mod.take() {
        validate_module_files(&m)?;
        container.modules.push(m);
    }

    // Default PROJ: to sys_name when absent
    if container.project.is_none() {
        container.project = Some(sys_name.to_string());
    }

    Ok(container)
}

// ────────────────────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────────────────────

/// Extract the value after `KEYWORD: ` (or `KEYWORD:\n`).
/// Matches on `"{keyword}:"` prefix to avoid false positives on keywords
/// that share a common prefix (e.g. "CONT" vs "CONTAINER").
fn keyword_value<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let prefix = format!("{keyword}:");
    if line.starts_with(prefix.as_str()) {
        let rest = &line[prefix.len()..];
        if rest.starts_with(' ') {
            Some(rest[1..].trim())
        } else if rest.is_empty() {
            Some("")
        } else {
            None
        }
    } else {
        None
    }
}

/// Split a `;`-separated list, trimming whitespace, dropping empty entries.
fn split_semi(s: &str) -> Vec<String> {
    s.split(';')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

fn flush_rule(rule: &mut Option<ArchSourceRule>, rules: &mut Vec<ArchSourceRule>) {
    if let Some(r) = rule.take() {
        if !r.id.is_empty() {
            rules.push(r);
        }
    }
}

fn flush_story(story: &mut Option<ArchSourceStory>, stories: &mut Vec<ArchSourceStory>) {
    if let Some(s) = story.take() {
        if !s.id.is_empty() {
            stories.push(s);
        }
    }
}

/// Enforce FILE:/FILES: mutual exclusion.
fn validate_module_files(m: &ArchSourceModule) -> Result<(), String> {
    if m.file.is_some() && !m.files.is_empty() {
        return Err(format!(
            "Module '{}': FILE: and FILES: are mutually exclusive — remove one",
            m.id
        ));
    }
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// Unit tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_system_arch_guide_multiline() {
        let content = "ARCH: 0.4\nSYS: myapp\nDESC: A system\nGUIDE:\n  line one\n  line two\nCONT: api\n";
        let src = parse_system_arch(content).unwrap();
        assert_eq!(src.system_name, "myapp");
        assert_eq!(src.container_ids, vec!["api"]);
        assert!(src.guidance.contains("line one"), "GUIDE multi-line should accumulate");
        assert!(src.guidance.contains("line two"), "GUIDE second line should be included");
    }

    #[test]
    fn parse_container_arch_file_files_mutual_exclusion() {
        let content = "ARCH: 0.4\nCONT: commands\nPATH: src/commands\n\nMOD: broken\nFILE: a.rs\nFILES: b.rs; c.rs\n";
        let result = parse_container_arch(content, "commands", "myapp");
        assert!(result.is_err(), "FILE: + FILES: together should be an error");
    }

    #[test]
    fn parse_system_arch_unknown_keyword_skipped() {
        let content = "ARCH: 0.4\nSYS: x\nDESC: test\nFUTURE_KEY: some value\nCONT: alpha\n";
        let src = parse_system_arch(content).unwrap();
        // Unknown keyword should not prevent normal parsing
        assert_eq!(src.container_ids, vec!["alpha"]);
    }

    #[test]
    fn parse_container_arch_proj_defaults_to_sys_name() {
        let content = "ARCH: 0.4\nCONT: api\nPATH: src\n";
        let container = parse_container_arch(content, "api", "myapp").unwrap();
        assert_eq!(container.project.as_deref(), Some("myapp"));
    }

    #[test]
    fn parse_system_arch_rule_block() {
        let content = "ARCH: 0.4\nSYS: x\nDESC: d\n\nRULE: schema-independence\nTYPE: no_dependency\nFROM: schema\nTO: commands; cli\nWHY: pure types\n";
        let src = parse_system_arch(content).unwrap();
        assert_eq!(src.rules.len(), 1);
        let r = &src.rules[0];
        assert_eq!(r.id, "schema-independence");
        assert_eq!(r.rule_type, "no_dependency");
        assert_eq!(r.from.as_deref(), Some("schema"));
        assert_eq!(r.to, vec!["commands", "cli"]);
        assert_eq!(r.reason.as_deref(), Some("pure types"));
    }
}
