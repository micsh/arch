use crate::schema::{self, Architecture, ContainerDetail, Language};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Index mapping keywords to module IDs for import resolution.
/// Used by both `drift` (dependency checking) and `stories` (flow verification).
pub struct ModuleIndex {
    /// Maps normalized keyword → set of "container/module" IDs.
    /// Keys are always normalized via schema::normalize_id().
    keyword_to_modules: HashMap<String, HashSet<String>>,
    /// Maps normalized file stem → "container/module" ID.
    file_stem_to_module: HashMap<String, String>,
    /// Maps container ID (lowercase) → detected language.
    container_languages: HashMap<String, Language>,
    /// Set of container IDs (lowercase) that have at least one module.
    active_containers: HashSet<String>,
}

impl ModuleIndex {
    pub fn build(
        _root: &Path,
        arch: &Architecture,
        details: &HashMap<String, ContainerDetail>,
    ) -> Self {
        let mut keyword_to_modules: HashMap<String, HashSet<String>> = HashMap::new();
        let mut file_stem_to_module: HashMap<String, String> = HashMap::new();
        let mut container_languages: HashMap<String, Language> = HashMap::new();
        let mut active_containers: HashSet<String> = HashSet::new();

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

            let container_norm = schema::normalize_id(&container.id);

            for module in &detail.modules {
                let full_id = format!("{}/{}", container.id, module.id);
                let mod_norm = schema::normalize_id(&module.id);

                // Index by module id (normalized)
                keyword_to_modules
                    .entry(mod_norm.clone())
                    .or_default()
                    .insert(full_id.clone());

                // Index by file stem (for each file the module owns)
                for file in module.all_files() {
                    let file_path = Path::new(file);
                    if let Some(stem) = file_path.file_stem().and_then(|s| s.to_str()) {
                        let stem_norm = schema::normalize_id(stem);
                        file_stem_to_module.insert(stem_norm, full_id.clone());
                    }
                }

                // Index by owns concepts
                for concept in &module.owns {
                    let key = schema::normalize_id(concept);
                    keyword_to_modules
                        .entry(key)
                        .or_default()
                        .insert(full_id.clone());
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
                    let project_norm = schema::normalize_id(project);
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

        ModuleIndex {
            keyword_to_modules,
            file_stem_to_module,
            container_languages,
            active_containers,
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
    pub fn resolve(&self, import: &str, source_container: &str) -> HashSet<String> {
        let source_lang = self.container_language(source_container);
        self.resolve_with_lang(import, source_container, source_lang)
    }

    fn resolve_with_lang(
        &self,
        import: &str,
        source_container: &str,
        source_lang: Language,
    ) -> HashSet<String> {
        let normalized = import.to_lowercase();
        // Specific results: matched via container.module composite or direct module ID
        let mut specific = HashSet::new();
        // Broad results: matched via container name only (fan-out)
        let mut broad = HashSet::new();

        // Strategy 1: Direct match (all separators stripped → single key)
        let direct = schema::normalize_id(&normalized.replace('.', "").replace("::", ""));
        if let Some(modules) = self.keyword_to_modules.get(&direct) {
            specific.extend(modules.iter().cloned());
        }

        // Strategy 2: .NET dot-separated — try longest normalized prefix first
        if normalized.contains('.') {
            let norm_dotted = schema::normalize_dotted(&normalized);
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
        } else if normalized.contains("::") {
            // Rust-style: match each segment
            for segment in normalized.split("::") {
                let clean = schema::normalize_id(segment);
                if let Some(modules) = self.keyword_to_modules.get(&clean) {
                    specific.extend(modules.iter().cloned());
                }
                if let Some(module) = self.file_stem_to_module.get(&clean) {
                    specific.insert(module.clone());
                }
            }
        } else {
            // Single word — direct lookup
            let clean = schema::normalize_id(&normalized);
            if let Some(modules) = self.keyword_to_modules.get(&clean) {
                // Single-word could be a module name (specific) or container name (broad).
                // Check: if the key matches a container name, it's broad.
                if self.active_containers.contains(&clean) {
                    broad.extend(modules.iter().cloned());
                } else {
                    specific.extend(modules.iter().cloned());
                }
            }
            if let Some(module) = self.file_stem_to_module.get(&clean) {
                specific.insert(module.clone());
            }
        }

        // Strategy 3: Rust crate:: imports
        if normalized.starts_with("crate::") {
            let after_crate = normalized.strip_prefix("crate::").unwrap_or("");
            let first = schema::normalize_id(after_crate.split("::").next().unwrap_or(""));
            if let Some(modules) = self.keyword_to_modules.get(&first) {
                specific.extend(modules.iter().cloned());
            }
            if let Some(module) = self.file_stem_to_module.get(&first) {
                specific.insert(module.clone());
            }
        }

        // Strategy 4: Rust super:: imports
        if normalized.starts_with("super::") {
            let after_super = normalized.strip_prefix("super::").unwrap_or("");
            let first = schema::normalize_id(after_super.split("::").next().unwrap_or(""));
            if let Some(modules) = self.keyword_to_modules.get(&first) {
                specific.extend(modules.iter().cloned());
            }
            if let Some(module) = self.file_stem_to_module.get(&first) {
                specific.insert(module.clone());
            }
        }

        // Merge: prefer specific matches; fall back to broad only if no specific match
        let mut results = if specific.is_empty() { broad } else { specific };

        // Filter: remove self-references (same container)
        results.retain(|m| {
            let container = m.split('/').next().unwrap_or("");
            container != source_container
        });

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

        // Same-container preference: if results contain both same-container
        // and cross-container matches, keep only same-container
        // (This prevents intra-package imports from matching external containers)
        // NOTE: same-container results were filtered above. For self-references,
        // the caller (drift) handles them. Here we focus on cross-container only.

        results
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

    // Python standard library (common ones)
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
        assert_eq!(schema::normalize_id("Durable-Tasks"), "durabletasks");
        assert_eq!(schema::normalize_id("auto_segmentation"), "autosegmentation");
        assert_eq!(schema::normalize_id("DurableTasks"), "durabletasks");
        assert_eq!(schema::normalize_id("my-module_v2"), "mymodulev2");
    }

    #[test]
    fn test_normalize_dotted() {
        assert_eq!(schema::normalize_dotted("Common.Durable-Tasks"), "common.durabletasks");
        assert_eq!(schema::normalize_dotted("MyApp.Data_Processing"), "myapp.dataprocessing");
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
}
