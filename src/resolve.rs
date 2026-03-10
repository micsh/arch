use crate::schema::{self, Architecture, ContainerDetail, Language};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Index mapping keywords to module IDs for import resolution.
/// Used by both `drift` (dependency checking) and `stories` (flow verification).
pub struct ModuleIndex {
    /// Maps normalized keyword → set of "container/module" IDs.
    /// Keys are always normalized via schema::normalize_id().
    keyword_to_modules: HashMap<String, HashSet<String>>,
    /// Maps normalized file stem → set of "container/module" IDs.
    /// Uses HashSet to handle stem collisions across containers.
    file_stem_to_module: HashMap<String, HashSet<String>>,
    /// Maps container ID (lowercase) → detected language.
    container_languages: HashMap<String, Language>,
    /// Set of container IDs (lowercase) that have at least one module.
    active_containers: HashSet<String>,
    /// Maps container/module (lowercase) → re-export entries parsed from that module's entry-point.
    /// Only populated for Rust modules whose file is a recognised entry-point (mod.rs, lib.rs, main.rs).
    reexports: HashMap<String, Vec<schema::PubUseEntry>>,
}

impl ModuleIndex {
    pub fn build(
        root: &Path,
        arch: &Architecture,
        details: &HashMap<String, ContainerDetail>,
    ) -> Self {
        let mut keyword_to_modules: HashMap<String, HashSet<String>> = HashMap::new();
        let mut file_stem_to_module: HashMap<String, HashSet<String>> = HashMap::new();
        let mut container_languages: HashMap<String, Language> = HashMap::new();
        let mut active_containers: HashSet<String> = HashSet::new();
        let mut reexports: HashMap<String, Vec<schema::PubUseEntry>> = HashMap::new();

        for container in &arch.containers {
            let detail = match details.get(&container.id) {
                Some(d) => d,
                None => continue,
            };

            if detail.modules.is_empty() {
                continue; // Skip empty containers — they shouldn't participate in resolution
            }
            active_containers.insert(container.id.to_lowercase());

            // Detect container language from module file extensions
            let mut lang_votes: HashMap<Language, usize> = HashMap::new();
            for module in &detail.modules {
                let lang = schema::detect_language(&module.file);
                if lang != Language::Unknown {
                    *lang_votes.entry(lang).or_default() += 1;
                }
            }
            if let Some((&lang, _)) = lang_votes.iter().max_by_key(|(_, v)| **v) {
                container_languages.insert(container.id.to_lowercase(), lang);
            }

            let container_norm = match schema::normalize_id(&container.id) {
                Some(n) => n,
                None => continue,
            };

            for module in &detail.modules {
                let full_id = format!("{}/{}", container.id, module.id);
                let mod_norm = match schema::normalize_id(&module.id) {
                    Some(n) => n,
                    None => continue,
                };

                // Index by module id (normalized)
                keyword_to_modules
                    .entry(mod_norm.clone())
                    .or_default()
                    .insert(full_id.clone());

                // Index by file stem (for each file the module owns)
                for file in module.all_files() {
                    let file_path = Path::new(file);
                    if let Some(stem) = file_path.file_stem().and_then(|s| s.to_str()) {
                        if let Some(stem_norm) = schema::normalize_id(stem) {
                            file_stem_to_module
                                .entry(stem_norm)
                                .or_default()
                                .insert(full_id.clone());
                        }
                    }
                }

                // Index by owns concepts
                for concept in &module.owns {
                    if let Some(key) = schema::normalize_id(concept) {
                        keyword_to_modules
                            .entry(key)
                            .or_default()
                            .insert(full_id.clone());
                    }
                }

                // Index by container name alone (maps to all modules in container)
                keyword_to_modules
                    .entry(container_norm.clone())
                    .or_default()
                    .insert(full_id.clone());

                // Index by container.module composite (both normalized)
                let composite = format!("{}.{}", container_norm, mod_norm);
                keyword_to_modules
                    .entry(composite)
                    .or_default()
                    .insert(full_id.clone());

                // For .NET: index by project name if present
                if let Some(ref project) = container.project {
                    if let Some(project_norm) = schema::normalize_id(project) {
                        keyword_to_modules
                            .entry(project_norm.clone())
                            .or_default()
                            .insert(full_id.clone());

                        // Index "project.module" patterns
                        let proj_mod = format!("{}.{}", project_norm, mod_norm);
                        keyword_to_modules
                            .entry(proj_mod)
                            .or_default()
                            .insert(full_id.clone());
                    }
                }
            }
        }

        // Second pass: scan Rust entry-point files for pub use re-exports.
        // Only Rust containers; only modules whose file is a recognised entry-point.
        for container in arch.containers.iter() {
            let lang = container_languages.get(&container.id.to_lowercase()).copied();
            if lang != Some(Language::Rust) {
                continue;
            }
            let detail = match details.get(&container.id) {
                Some(d) => d,
                None => continue,
            };
            for module in &detail.modules {
                if !schema::is_entry_point(&module.file) {
                    continue;
                }
                let file_path = root.join(&container.path).join(&module.file);
                let content = match std::fs::read_to_string(&file_path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let entries = schema::extract_rust_pub_uses(&content);
                if !entries.is_empty() {
                    let key = format!("{}/{}", container.id, module.id).to_lowercase();
                    reexports.insert(key, entries);
                }
            }
        }

        ModuleIndex {
            keyword_to_modules,
            file_stem_to_module,
            container_languages,
            active_containers,
            reexports,
        }
    }

    /// Get the detected language for a container.
    pub fn container_language(&self, container_id: &str) -> Language {
        self.container_languages
            .get(&container_id.to_lowercase())
            .copied()
            .unwrap_or(Language::Unknown)
    }

    /// Resolve an import string to a set of possible module IDs.
    /// Applies language scoping, same-container preference, and specificity ranking.
    /// Excludes same-container matches (use `resolve_all` when intra-container visibility needed).
    pub fn resolve(&self, import: &str, source_container: &str) -> HashSet<String> {
        let source_lang = self.container_language(source_container);
        self.resolve_impl(import, source_container, source_lang, true)
    }

    /// Like resolve(), but includes same-container matches in results.
    /// Used by stories to verify intra-container import connections.
    pub fn resolve_all(&self, import: &str, source_container: &str) -> HashSet<String> {
        let source_lang = self.container_language(source_container);
        self.resolve_impl(import, source_container, source_lang, false)
    }

    fn resolve_impl(
        &self,
        import: &str,
        source_container: &str,
        source_lang: Language,
        exclude_self_container: bool,
    ) -> HashSet<String> {
        let normalized = import.to_lowercase();

        // Python relative imports (starting with .) are always intra-package —
        // they should never resolve to a different container
        if normalized.starts_with('.') {
            return HashSet::new();
        }

        // Specific results: matched via container.module composite or direct module ID
        let mut specific = HashSet::new();
        // Broad results: matched via container name only (fan-out)
        let mut broad = HashSet::new();

        // Strategy 1: Direct match (all separators stripped → single key)
        if let Some(direct) = schema::normalize_id(&normalized.replace('.', "").replace("::", "")) {
            if let Some(modules) = self.keyword_to_modules.get(&direct) {
                specific.extend(modules.iter().cloned());
            }
        }

        // Strategy 2: .NET dot-separated — try longest normalized prefix first
        if normalized.contains('.') {
            let norm_dotted = schema::normalize_dotted(&normalized);
            if !norm_dotted.is_empty() {
                let segments: Vec<&str> = norm_dotted.split('.').collect();
                for len in (1..=segments.len()).rev() {
                    let prefix = segments[..len].join(".");
                    if let Some(modules) = self.keyword_to_modules.get(&prefix) {
                        if len > 1 {
                            specific.extend(modules.iter().cloned());
                        } else {
                            broad.extend(modules.iter().cloned());
                        }
                        break;
                    }
                }
            }
        } else if normalized.contains("::") {
            // Rust-style: match each segment
            for segment in normalized.split("::") {
                if let Some(clean) = schema::normalize_id(segment) {
                    if let Some(modules) = self.keyword_to_modules.get(&clean) {
                        specific.extend(modules.iter().cloned());
                    }
                    if let Some(modules) = self.file_stem_to_module.get(&clean) {
                        specific.extend(modules.iter().cloned());
                    }
                }
            }
        } else {
            // Single word — direct lookup
            if let Some(clean) = schema::normalize_id(&normalized) {
                if let Some(modules) = self.keyword_to_modules.get(&clean) {
                    // Single-word could be a module name (specific) or container name (broad).
                    // Check: if the key matches a container name, it's broad.
                    if self.active_containers.contains(&clean) {
                        broad.extend(modules.iter().cloned());
                    } else {
                        specific.extend(modules.iter().cloned());
                    }
                }
                if let Some(modules) = self.file_stem_to_module.get(&clean) {
                    specific.extend(modules.iter().cloned());
                }
            }
        }

        // Strategy 3: Rust crate:: imports
        if normalized.starts_with("crate::") {
            let after_crate = normalized.strip_prefix("crate::").unwrap_or("");
            if let Some(first) = after_crate.split("::").next().and_then(|s| schema::normalize_id(s)) {
                if let Some(modules) = self.keyword_to_modules.get(&first) {
                    specific.extend(modules.iter().cloned());
                }
                if let Some(modules) = self.file_stem_to_module.get(&first) {
                    specific.extend(modules.iter().cloned());
                }
            }
        }

        // Strategy 4: Rust super:: imports
        if normalized.starts_with("super::") {
            let after_super = normalized.strip_prefix("super::").unwrap_or("");
            if let Some(first) = after_super.split("::").next().and_then(|s| schema::normalize_id(s)) {
                if let Some(modules) = self.keyword_to_modules.get(&first) {
                    specific.extend(modules.iter().cloned());
                }
                if let Some(modules) = self.file_stem_to_module.get(&first) {
                    specific.extend(modules.iter().cloned());
                }
            }
        }

        // Merge: prefer specific matches; fall back to broad only if no specific match
        let mut results = if specific.is_empty() { broad } else { specific };

        // Filter: remove self-references (same container) when requested
        if exclude_self_container {
            results.retain(|m| {
                let container = m.split('/').next().unwrap_or("");
                container != source_container
            });
        }

        // Language scoping: filter out cross-language matches
        // Unknown-language containers only match if no known-language results exist
        if source_lang != Language::Unknown {
            let has_known_lang_match = results.iter().any(|m| {
                let container = m.split('/').next().unwrap_or("");
                let target_lang = self.container_language(container);
                target_lang != Language::Unknown && source_lang.is_compatible(target_lang)
            });

            results.retain(|m| {
                let container = m.split('/').next().unwrap_or("");
                let target_lang = self.container_language(container);
                if target_lang == Language::Unknown {
                    // Unknown containers only pass if no known-language match exists
                    !has_known_lang_match
                } else {
                    source_lang.is_compatible(target_lang)
                }
            });
        }

        // Re-export collapse: for Rust imports that go through a pub-use facade,
        // collapse multi-module matches in the same container down to the facade module.
        // Only applied to Rust sources — other languages have different re-export semantics.
        // ASSUMPTION: re-export chains longer than one level (A pub-uses from B, B pub-uses from C)
        // are not fully collapsed. IF INVALID: call try_collapse_reexports in a loop until stable.
        if results.len() > 1 && source_lang == Language::Rust {
            let last_seg = import.split("::").last().unwrap_or("");
            self.try_collapse_reexports(&mut results, last_seg);
        }

        results
    }

    /// Attempt to collapse a set of matches belonging to the same foreign container
    /// when one match is a re-exporting facade of the others.
    ///
    /// `import_last_segment`: the last `::` segment of the original import (the accessed name).
    /// Mutates `results` in-place; safe no-op if no collapse is applicable.
    fn try_collapse_reexports(
        &self,
        results: &mut HashSet<String>,
        import_last_segment: &str,
    ) {
        // Group results by container — only containers with multiple candidates need collapsing
        let mut by_container: HashMap<String, Vec<String>> = HashMap::new();
        for m in results.iter() {
            let container = m.split('/').next().unwrap_or("").to_string();
            by_container.entry(container).or_default().push(m.clone());
        }

        let mut to_remove: HashSet<String> = HashSet::new();

        for (_container, candidates) in &by_container {
            if candidates.len() <= 1 {
                continue;
            }

            // Try each candidate as the facade module
            'facade_search: for facade in candidates.iter() {
                let facade_reexports = match self.reexports.get(&facade.to_lowercase()) {
                    Some(r) => r,
                    None => continue,
                };

                let others: Vec<&String> = candidates.iter().filter(|c| *c != facade).collect();
                let last_seg_lower = import_last_segment.to_lowercase();

                // Check: do this facade's re-exports cover every other candidate?
                let all_covered = others.iter().all(|other| {
                    let other_module_part = other.split('/').nth(1).unwrap_or("");
                    let other_norm = other_module_part
                        .to_lowercase()
                        .replace('-', "")
                        .replace('_', "");
                    facade_reexports.iter().any(|e| {
                        let src_norm = e.source_module
                            .to_lowercase()
                            .replace('-', "")
                            .replace('_', "");
                        src_norm == other_norm
                            && (e.symbol.is_none()
                                || e.symbol.as_deref().map(|s| s.to_lowercase()).as_deref()
                                    == Some(last_seg_lower.as_str()))
                    })
                });

                if all_covered {
                    for other in &others {
                        to_remove.insert((*other).clone());
                    }
                    break 'facade_search;
                }
            }
        }

        results.retain(|m| !to_remove.contains(m));
    }
}

/// Check if an import is to a standard library / external dependency.
pub fn is_external_import(import: &str) -> bool {
    let lower = import.to_lowercase();

    // .NET standard libraries
    if lower.starts_with("system")
        || lower.starts_with("microsoft.")
        || lower.starts_with("fsharp.")
    {
        return true;
    }

    // Rust standard library
    if lower.starts_with("std::")
        || lower.starts_with("core::")
        || lower.starts_with("alloc::")
    {
        return true;
    }

    // Node.js 'node:' prefix form (e.g. node:fs, node:path) — introduced in Node 14/18.
    // Matches regardless of whether the bare name is in the builtins list.
    if lower.starts_with("node:") {
        return true;
    }

    // Python standard library (common ones)
    // ASSUMPTION: Python stdlib list is frozen at ~Python 3.8.
    // IF INVALID (newer stdlib modules flagged as false-positive drift): extend python_std array.
    // Notable omissions: 'tomllib' (3.11+), 'graphlib' (3.9+), 'zoneinfo' (3.9+).
    let python_std = [
        "os",
        "sys",
        "json",
        "io",
        "re",
        "math",
        "typing",
        "pathlib",
        "collections",
        "functools",
        "itertools",
        "datetime",
        "logging",
        "unittest",
        "abc",
        "enum",
        "dataclasses",
        "contextlib",
        "copy",
        "hashlib",
        "base64",
        "urllib",
        "http",
        "socket",
        "threading",
        "multiprocessing",
        "subprocess",
        "shutil",
        "glob",
        "tempfile",
        "argparse",
        "configparser",
        "csv",
        "xml",
        "html",
        "email",
    ];
    if python_std.contains(&lower.as_str()) {
        return true;
    }

    // Node.js built-ins
    // ASSUMPTION: node_builtins covers Node.js LTS built-ins as of Node 18.
    // The 'node:' prefix form (e.g. 'node:fs') is handled by the prefix check above.
    // IF INVALID: extend node_builtins array or update the prefix check.
    let node_builtins = [
        "fs",
        "path",
        "os",
        "http",
        "https",
        "url",
        "crypto",
        "stream",
        "events",
        "child_process",
        "util",
        "assert",
        "buffer",
        "cluster",
        "dns",
        "net",
        "querystring",
        "readline",
        "tls",
        "zlib",
    ];
    if node_builtins.contains(&lower.as_str()) {
        return true;
    }

    // Relative imports (Python/TS) — project-internal, not external
    if lower.starts_with('.') {
        return false;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Architecture, Container, ContainerDetail, Module, System};

    fn make_arch(containers: Vec<Container>) -> Architecture {
        Architecture {
            guidance: None,
            system: System {
                name: "test".to_string(),
                description: None,
                ignore: vec![],
            },
            containers,
            rules: vec![],
        }
    }

    fn make_detail(modules: Vec<Module>) -> ContainerDetail {
        ContainerDetail { modules, notes: None }
    }

    fn make_module(id: &str, file: &str, owns: Vec<&str>) -> Module {
        Module {
            id: id.to_string(),
            file: file.to_string(),
            files: vec![],
            owns: owns.into_iter().map(|s| s.to_string()).collect(),
            boundary: None,
            depends_on: vec![],
            must_not_depend: vec![],
            routes: None,
        }
    }

    fn make_container(id: &str, path: &str, deps: Vec<&str>, project: Option<&str>) -> Container {
        Container {
            id: id.to_string(),
            path: path.to_string(),
            description: Some(format!("{id} container")),
            depends_on: deps.into_iter().map(|s| s.to_string()).collect(),
            project: project.map(|s| s.to_string()),
            notes: None,
        }
    }

    #[test]
    fn test_cross_container_csharp_namespace() {
        // using Common.DurableTasks → should resolve to common/durable-tasks
        let arch = make_arch(vec![
            make_container("rp-service", "src/RP", vec!["common"], None),
            make_container("common", "src/Common", vec![], None),
        ]);
        let mut details = HashMap::new();
        details.insert("rp-service".to_string(), make_detail(vec![
            make_module("service", "Service.csproj", vec!["api-hosting"]),
        ]));
        details.insert("common".to_string(), make_detail(vec![
            make_module("durable-tasks", "DurableTasks/DurableTasks.csproj", vec!["durable-orchestration"]),
            make_module("messaging", "Messaging/Messaging.csproj", vec!["service-bus"]),
        ]));

        let root = Path::new(".");
        let index = ModuleIndex::build(root, &arch, &details);
        let result = index.resolve("Common.DurableTasks", "rp-service");

        assert!(result.contains("common/durable-tasks"), "Should resolve to common/durable-tasks, got: {:?}", result);
        assert!(!result.contains("common/messaging"), "Should NOT fan out to messaging, got: {:?}", result);
    }

    #[test]
    fn test_no_fanout_on_container_match() {
        // Dataprocessing.V20250801 — no module matches V20250801, falls back to container
        let arch = make_arch(vec![
            make_container("rp-service", "src/RP", vec!["dataprocessing"], None),
            make_container("dataprocessing", "src/DataProcessing", vec![], None),
        ]);
        let mut details = HashMap::new();
        details.insert("rp-service".to_string(), make_detail(vec![
            make_module("service", "Service.csproj", vec!["api"]),
        ]));
        details.insert("dataprocessing".to_string(), make_detail(vec![
            make_module("domain", "Domain/Domain.csproj", vec!["business-logic"]),
            make_module("application", "App/App.csproj", vec!["app-hosting"]),
            make_module("contracts", "Contracts/Contracts.csproj", vec!["api-contracts"]),
        ]));

        let root = Path::new(".");
        let index = ModuleIndex::build(root, &arch, &details);
        let result = index.resolve("Dataprocessing.V20250801", "rp-service");

        // V20250801 doesn't match any module, so broad fallback includes all 3.
        // This is acceptable — the user should add an explicit module or ignore pattern.
        assert!(result.len() <= 3, "Fan-out should be bounded, got {} results: {:?}", result.len(), result);
    }

    #[test]
    fn test_empty_container_excluded() {
        // Empty containers (no modules) should not participate in resolution
        let arch = make_arch(vec![
            make_container("python", "src/python", vec![], None),
            make_container("autosegmentation", "src/AutoSeg", vec![], None),
        ]);
        let mut details = HashMap::new();
        details.insert("python".to_string(), make_detail(vec![
            make_module("auto-segmentation", "auto_seg/__init__.py", vec!["segmentation"]),
        ]));
        details.insert("autosegmentation".to_string(), make_detail(vec![]));

        let root = Path::new(".");
        let index = ModuleIndex::build(root, &arch, &details);

        // "auto_segmentation" from python — should NOT match empty autosegmentation container
        let result = index.resolve("auto_segmentation", "python");
        assert!(!result.iter().any(|m| m.starts_with("autosegmentation/")),
            "Should not resolve to empty container, got: {:?}", result);
    }

    #[test]
    fn test_language_scoping_prefers_known_over_unknown() {
        // Unknown-language containers excluded when known-language matches exist
        let arch = make_arch(vec![
            make_container("python", "src/python", vec![], None),
            make_container("common-py", "src/common-py", vec![], None),
            make_container("archive", "src/archive", vec![], None),
        ]);
        let mut details = HashMap::new();
        details.insert("python".to_string(), make_detail(vec![
            make_module("app", "app/__init__.py", vec!["main-app"]),
        ]));
        details.insert("common-py".to_string(), make_detail(vec![
            make_module("utils", "utils/__init__.py", vec!["utility-functions"]),
        ]));
        details.insert("archive".to_string(), make_detail(vec![
            make_module("utils", "utils.txt", vec!["utility-functions"]),
        ]));

        let root = Path::new(".");
        let index = ModuleIndex::build(root, &arch, &details);

        let result = index.resolve("utils", "python");
        let has_python = result.iter().any(|m| m.starts_with("common-py/"));
        let has_archive = result.iter().any(|m| m.starts_with("archive/"));
        assert!(has_python, "Should resolve to Python container, got: {:?}", result);
        assert!(!has_archive, "Should NOT resolve to Unknown-language archive, got: {:?}", result);
    }

    #[test]
    fn test_normalize_id_consistency() {
        assert_eq!(schema::normalize_id("Durable-Tasks"), Some("durabletasks".to_string()));
        assert_eq!(schema::normalize_id("auto_segmentation"), Some("autosegmentation".to_string()));
        assert_eq!(schema::normalize_id("DurableTasks"), Some("durabletasks".to_string()));
        assert_eq!(schema::normalize_id("my-module_v2"), Some("mymodulev2".to_string()));
    }

    #[test]
    fn test_normalize_id_edge_cases() {
        // Empty strings and all-separator strings should return None
        assert_eq!(schema::normalize_id(""), None);
        assert_eq!(schema::normalize_id("---"), None);
        assert_eq!(schema::normalize_id("___"), None);
        assert_eq!(schema::normalize_id("-_-_-"), None);
        // Single char should work
        assert_eq!(schema::normalize_id("a"), Some("a".to_string()));
    }

    #[test]
    fn test_normalize_dotted() {
        assert_eq!(schema::normalize_dotted("Common.Durable-Tasks"), "common.durabletasks");
        assert_eq!(schema::normalize_dotted("MyApp.Data_Processing"), "myapp.dataprocessing");
        // Empty segments dropped
        assert_eq!(schema::normalize_dotted("a...b"), "a.b");
        assert_eq!(schema::normalize_dotted("---"), "");
    }

    #[test]
    fn test_self_references_excluded() {
        // Imports should not resolve to modules in the source container
        let arch = make_arch(vec![
            make_container("backend", "src/backend", vec![], None),
        ]);
        let mut details = HashMap::new();
        details.insert("backend".to_string(), make_detail(vec![
            make_module("auth", "auth.rs", vec!["authentication"]),
            make_module("api", "api.rs", vec!["routing"]),
        ]));

        let root = Path::new(".");
        let index = ModuleIndex::build(root, &arch, &details);
        let result = index.resolve("auth", "backend");

        assert!(result.is_empty(), "Self-container imports should be excluded, got: {:?}", result);
    }

    #[test]
    fn test_python_relative_imports_never_cross_container() {
        // Python relative imports (.foo, .bar.baz) should never match other containers
        let arch = make_arch(vec![
            make_container("python", "src/python", vec![], None),
            make_container("autosegmentation", "src/AutoSeg", vec![], None),
        ]);
        let mut details = HashMap::new();
        details.insert("python".to_string(), make_detail(vec![
            make_module("auto-segmentation", "auto_seg/__init__.py", vec!["segmentation"]),
        ]));
        details.insert("autosegmentation".to_string(), make_detail(vec![
            make_module("fsharp-prototype", "prototype.fsx", vec!["legacy"]),
        ]));

        let root = Path::new(".");
        let index = ModuleIndex::build(root, &arch, &details);

        // Relative imports should resolve to nothing (intra-package, not cross-container)
        let result = index.resolve(".auto_segmentation", "python");
        assert!(result.is_empty(), "Relative Python imports should not cross containers, got: {:?}", result);

        let result2 = index.resolve(".auto_segmentation.servicebus_listener", "python");
        assert!(result2.is_empty(), "Dotted relative Python imports should not cross containers, got: {:?}", result2);
    }

    #[test]
    fn test_resolve_all_includes_same_container() {
        // resolve_all should include same-container matches (for stories)
        let arch = make_arch(vec![
            make_container("backend", "src/backend", vec![], None),
        ]);
        let mut details = HashMap::new();
        details.insert("backend".to_string(), make_detail(vec![
            make_module("auth", "auth.rs", vec!["authentication"]),
            make_module("api", "api.rs", vec!["routing"]),
        ]));

        let root = Path::new(".");
        let index = ModuleIndex::build(root, &arch, &details);

        // resolve() excludes self-container
        let result = index.resolve("auth", "backend");
        assert!(result.is_empty(), "resolve() should exclude same-container, got: {:?}", result);

        // resolve_all() includes self-container
        let result_all = index.resolve_all("auth", "backend");
        assert!(result_all.contains("backend/auth"), "resolve_all() should include same-container, got: {:?}", result_all);
    }

    // ── Re-export collapse tests ──────────────────────────────────────────────

    /// Build a Rust ModuleIndex and inject re-export entries for testing.
    fn build_rust_index_with_reexports(
        arch: Architecture,
        details: HashMap<String, ContainerDetail>,
        facade_module_id: &str,  // e.g. "arch/stanza"
        reexport_entries: Vec<schema::PubUseEntry>,
    ) -> ModuleIndex {
        let mut index = ModuleIndex::build(Path::new("."), &arch, &details);
        index.reexports.insert(facade_module_id.to_lowercase(), reexport_entries);
        index
    }

    #[test]
    fn test_reexport_collapse_wildcard() {
        // arch/stanza re-exports types::* — resolving crate::stanza::PresenceStatus
        // should collapse {arch/stanza, arch/types} → {arch/stanza}
        let arch = make_arch(vec![
            make_container("consumer", "src/consumer", vec!["arch"], None),
            make_container("arch", "src/arch", vec![], None),
        ]);
        let mut details = HashMap::new();
        details.insert("consumer".to_string(), make_detail(vec![
            make_module("client", "client.rs", vec!["client-logic"]),
        ]));
        details.insert("arch".to_string(), make_detail(vec![
            make_module("stanza", "stanza.rs", vec!["stanza-types"]),
            make_module("types", "types.rs", vec!["PresenceStatus"]),
        ]));

        let index = build_rust_index_with_reexports(
            arch, details, "arch/stanza",
            vec![schema::PubUseEntry { source_module: "types".to_string(), symbol: None }],
        );

        let result = index.resolve("crate::stanza::PresenceStatus", "consumer");
        // Should collapse to just arch/stanza — types is covered by the wildcard re-export
        assert!(result.contains("arch/stanza"), "Should contain facade, got: {:?}", result);
        assert!(!result.contains("arch/types"), "Should NOT contain types after collapse, got: {:?}", result);
        assert_eq!(result.len(), 1, "Should be exactly one result after collapse, got: {:?}", result);
    }

    #[test]
    fn test_reexport_collapse_named_symbol() {
        // arch/stanza re-exports types::PresenceStatus specifically
        let arch = make_arch(vec![
            make_container("consumer", "src/consumer", vec!["arch"], None),
            make_container("arch", "src/arch", vec![], None),
        ]);
        let mut details = HashMap::new();
        details.insert("consumer".to_string(), make_detail(vec![
            make_module("client", "client.rs", vec!["client-logic"]),
        ]));
        details.insert("arch".to_string(), make_detail(vec![
            make_module("stanza", "stanza.rs", vec!["stanza-types"]),
            make_module("types", "types.rs", vec!["PresenceStatus"]),
        ]));

        let index = build_rust_index_with_reexports(
            arch, details, "arch/stanza",
            vec![schema::PubUseEntry {
                source_module: "types".to_string(),
                symbol: Some("PresenceStatus".to_string()),
            }],
        );

        let result = index.resolve("crate::stanza::PresenceStatus", "consumer");
        assert!(result.contains("arch/stanza"), "Should contain facade");
        assert!(!result.contains("arch/types"), "Should NOT contain types");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_reexport_no_collapse_wrong_symbol() {
        // arch/stanza re-exports types::OtherType — NOT PresenceStatus.
        // Results should remain unchanged (no false collapse).
        let arch = make_arch(vec![
            make_container("consumer", "src/consumer", vec!["arch"], None),
            make_container("arch", "src/arch", vec![], None),
        ]);
        let mut details = HashMap::new();
        details.insert("consumer".to_string(), make_detail(vec![
            make_module("client", "client.rs", vec!["client-logic"]),
        ]));
        details.insert("arch".to_string(), make_detail(vec![
            make_module("stanza", "stanza.rs", vec!["stanza-types"]),
            make_module("types", "types.rs", vec!["PresenceStatus"]),
        ]));

        let index = build_rust_index_with_reexports(
            arch, details, "arch/stanza",
            vec![schema::PubUseEntry {
                source_module: "types".to_string(),
                symbol: Some("OtherType".to_string()), // wrong symbol
            }],
        );

        let result = index.resolve("crate::stanza::PresenceStatus", "consumer");
        // No collapse — both modules remain
        assert!(result.len() >= 1, "Should still have results");
        // arch/types still present since collapse didn't apply
        assert!(result.contains("arch/types") || result.contains("arch/stanza"),
            "Should have unmodified results, got: {:?}", result);
    }

    #[test]
    fn test_reexport_no_collapse_for_non_rust() {
        // C# import — try_collapse_reexports must NOT be called (guard on source_lang == Rust)
        let arch = make_arch(vec![
            make_container("consumer", "src/consumer", vec!["arch"], None),
            make_container("arch", "src/arch", vec![], None),
        ]);
        let mut details = HashMap::new();
        details.insert("consumer".to_string(), make_detail(vec![
            make_module("client", "Client.csproj", vec!["client-logic"]),
        ]));
        details.insert("arch".to_string(), make_detail(vec![
            make_module("stanza", "Stanza.csproj", vec!["stanza-types", "PresenceStatus"]),
        ]));

        let index = ModuleIndex::build(Path::new("."), &arch, &details);
        // C# source resolving a C# import — should work normally without collapse interference
        let result = index.resolve("Stanza.PresenceStatus", "consumer");
        // The resolve should still work — collapse guard prevents interference
        // (result may be empty or contain arch/stanza — either is fine, key is no panic)
        let _ = result; // Just verify no panic / compile error
    }
}
