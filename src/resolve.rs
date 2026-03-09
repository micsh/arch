use crate::schema::{Architecture, ContainerDetail, Language};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Index mapping keywords to module IDs for import resolution.
/// Used by both `drift` (dependency checking) and `stories` (flow verification).
pub struct ModuleIndex {
    /// Maps lowercase keyword → set of "container/module" IDs
    keyword_to_modules: HashMap<String, HashSet<String>>,
    /// Maps file stem (without extension) → "container/module" ID
    file_stem_to_module: HashMap<String, String>,
    /// Maps container ID → detected language
    container_languages: HashMap<String, Language>,
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

        for container in &arch.containers {
            let detail = match details.get(&container.id) {
                Some(d) => d,
                None => continue,
            };

            // Detect container language from module file extensions
            let mut lang_votes: HashMap<Language, usize> = HashMap::new();
            for module in &detail.modules {
                let lang = crate::schema::detect_language(&module.file);
                if lang != Language::Unknown {
                    *lang_votes.entry(lang).or_default() += 1;
                }
            }
            if let Some((&lang, _)) = lang_votes.iter().max_by_key(|(_, v)| **v) {
                container_languages.insert(container.id.to_lowercase(), lang);
            }

            let container_key = container.id.to_lowercase();

            for module in &detail.modules {
                let full_id = format!("{}/{}", container.id, module.id);
                let mod_normalized =
                    module.id.to_lowercase().replace('-', "").replace('_', "");

                // Index by module id (raw lowercase)
                keyword_to_modules
                    .entry(module.id.to_lowercase())
                    .or_default()
                    .insert(full_id.clone());

                // Index by module id (normalized — hyphens/underscores stripped)
                if mod_normalized != module.id.to_lowercase() {
                    keyword_to_modules
                        .entry(mod_normalized.clone())
                        .or_default()
                        .insert(full_id.clone());
                }

                // Index by file stem (for each file the module owns)
                for file in module.all_files() {
                    let file_path = Path::new(file);
                    if let Some(stem) = file_path.file_stem().and_then(|s| s.to_str()) {
                        let stem_lower = stem.to_lowercase();
                        file_stem_to_module
                            .insert(stem_lower.clone(), full_id.clone());
                        // Also insert normalized stem
                        let stem_normalized =
                            stem_lower.replace('-', "").replace('_', "").replace('.', "");
                        if stem_normalized != stem_lower {
                            file_stem_to_module
                                .insert(stem_normalized, full_id.clone());
                        }
                    }
                }

                // Index by owns concepts
                for concept in &module.owns {
                    let key = concept.to_lowercase().replace('-', "").replace('_', "");
                    keyword_to_modules
                        .entry(key)
                        .or_default()
                        .insert(full_id.clone());
                }

                // Index by container name (maps to all modules in container)
                keyword_to_modules
                    .entry(container_key.clone())
                    .or_default()
                    .insert(full_id.clone());

                // Index by container.module composite (normalized)
                let container_mod =
                    format!("{}.{}", container_key, mod_normalized);
                keyword_to_modules
                    .entry(container_mod)
                    .or_default()
                    .insert(full_id.clone());

                // For .NET: index by project name if present
                if let Some(ref project) = container.project {
                    let project_key = project.to_lowercase();
                    keyword_to_modules
                        .entry(project_key.clone())
                        .or_default()
                        .insert(full_id.clone());

                    // Index "project.module" patterns (both raw and normalized)
                    let combined =
                        format!("{}.{}", project_key, mod_normalized);
                    keyword_to_modules
                        .entry(combined)
                        .or_default()
                        .insert(full_id.clone());
                }
            }
        }

        ModuleIndex {
            keyword_to_modules,
            file_stem_to_module,
            container_languages,
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
    /// When `source_lang` is provided, results are filtered to containers
    /// with a compatible language (prevents Python→C# false positives).
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
        let mut results = HashSet::new();
        let mut specific_match = false;

        // Strategy 1: Direct match (all separators stripped)
        let direct = normalized
            .replace('.', "")
            .replace("::", "")
            .replace('-', "")
            .replace('_', "");
        if let Some(modules) = self.keyword_to_modules.get(&direct) {
            results.extend(modules.iter().cloned());
        }

        // Strategy 2: .NET dot-separated — try longest prefix first
        if normalized.contains('.') {
            let segments: Vec<&str> = normalized.split('.').collect();
            for len in (1..=segments.len()).rev() {
                let prefix: String = segments[..len]
                    .iter()
                    .map(|s| s.replace('-', "").replace('_', ""))
                    .collect::<Vec<_>>()
                    .join(".");
                if let Some(modules) = self.keyword_to_modules.get(&prefix) {
                    if len > 1 {
                        // Multi-segment match is specific (container.module)
                        specific_match = true;
                        results.extend(modules.iter().cloned());
                    } else {
                        // Single-segment match is a container-level fallback
                        results.extend(modules.iter().cloned());
                    }
                    break;
                }
            }
        } else if normalized.contains("::") {
            // Rust-style: match each segment
            for segment in normalized.split("::") {
                let clean = segment.replace('-', "").replace('_', "");
                if let Some(modules) = self.keyword_to_modules.get(&clean) {
                    results.extend(modules.iter().cloned());
                }
                if let Some(module) = self.file_stem_to_module.get(&clean) {
                    results.insert(module.clone());
                }
            }
        } else {
            // Single word — direct lookup
            let clean = normalized.replace('-', "").replace('_', "");
            if let Some(modules) = self.keyword_to_modules.get(&clean) {
                results.extend(modules.iter().cloned());
            }
            if let Some(module) = self.file_stem_to_module.get(&clean) {
                results.insert(module.clone());
            }
        }

        // Strategy 3: Rust crate:: imports
        if normalized.starts_with("crate::") {
            let after_crate = normalized.strip_prefix("crate::").unwrap_or("");
            let first = after_crate.split("::").next().unwrap_or("");
            if let Some(modules) = self.keyword_to_modules.get(first) {
                results.extend(modules.iter().cloned());
            }
            if let Some(module) = self.file_stem_to_module.get(first) {
                results.insert(module.clone());
            }
        }

        // Strategy 4: Rust super:: imports
        if normalized.starts_with("super::") {
            let after_super = normalized.strip_prefix("super::").unwrap_or("");
            let first = after_super.split("::").next().unwrap_or("");
            if let Some(modules) = self.keyword_to_modules.get(first) {
                results.extend(modules.iter().cloned());
            }
            if let Some(module) = self.file_stem_to_module.get(first) {
                results.insert(module.clone());
            }
        }

        // Filter: keep cross-container and same-container results
        results.retain(|m| {
            let container = m.split('/').next().unwrap_or("");
            container == source_container || !m.starts_with(source_container)
        });

        // Best-match: if we have a specific match (container.module), discard
        // container-level matches from the same container to avoid fanout
        if specific_match && results.len() > 1 {
            let specific_containers: HashSet<&str> = results
                .iter()
                .filter(|m| {
                    let parts: Vec<&str> = m.split('/').collect();
                    parts.len() == 2
                })
                .filter_map(|m| m.split('/').next())
                .collect();
            // Keep only the specifically-matched modules in those containers
            if !specific_containers.is_empty() {
                // Don't discard — the specific_match flag means we already
                // matched at container.module level, so results ARE specific
            }
        }

        // Language scoping: filter out cross-language matches
        if source_lang != Language::Unknown {
            results.retain(|m| {
                let container = m.split('/').next().unwrap_or("");
                if container == source_container {
                    return true; // same container always allowed
                }
                let target_lang = self.container_language(container);
                target_lang == Language::Unknown || source_lang.is_compatible(target_lang)
            });
        }

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
