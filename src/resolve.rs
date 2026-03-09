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

            for module in &detail.modules {
                let full_id = format!("{}/{}", container.id, module.id);

                // Index by module id
                keyword_to_modules
                    .entry(module.id.to_lowercase())
                    .or_default()
                    .insert(full_id.clone());

                // Index by file stem
                let file_path = Path::new(&module.file);
                if let Some(stem) = file_path.file_stem().and_then(|s| s.to_str()) {
                    file_stem_to_module.insert(stem.to_lowercase(), full_id.clone());
                }

                // Index by owns concepts
                for concept in &module.owns {
                    let key = concept.to_lowercase().replace('-', "").replace('_', "");
                    keyword_to_modules
                        .entry(key)
                        .or_default()
                        .insert(full_id.clone());
                }

                // Index by container.module pattern
                let container_key = container.id.to_lowercase();
                keyword_to_modules
                    .entry(container_key.clone())
                    .or_default()
                    .insert(full_id.clone());

                // For .NET: index by project name if present
                if let Some(ref project) = container.project {
                    let project_key = project.to_lowercase();
                    keyword_to_modules
                        .entry(project_key.clone())
                        .or_default()
                        .insert(full_id.clone());

                    // Also index "project.module" patterns
                    let combined = format!("{}.{}", project_key, module.id.to_lowercase());
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

        // Strategy 1: Direct match (lowercased, separators removed)
        if let Some(modules) = self
            .keyword_to_modules
            .get(&normalized.replace('.', "").replace("::", ""))
        {
            results.extend(modules.iter().cloned());
        }

        // Strategy 2: .NET dot-separated longest-match prefix
        if normalized.contains('.') {
            let segments: Vec<&str> = normalized.split('.').collect();
            for len in (1..=segments.len()).rev() {
                let prefix = segments[..len].join(".");
                if let Some(modules) = self.keyword_to_modules.get(&prefix) {
                    results.extend(modules.iter().cloned());
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
