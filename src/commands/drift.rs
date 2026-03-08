use crate::imports;
use crate::schema::{Architecture, ContainerDetail};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// A single drift finding.
struct DriftItem {
    module_id: String,
    file: String,
    import_raw: String,
    line_number: usize,
    target_module: String,
    kind: DriftKind,
}

enum DriftKind {
    /// Import exists but not declared in depends_on
    Undeclared,
    /// Import exists and is in must_not_depend
    Forbidden,
}

/// Index mapping keywords to module IDs for import resolution.
struct ModuleIndex {
    /// Maps lowercase keyword → set of "container/module" IDs
    keyword_to_modules: HashMap<String, HashSet<String>>,
    /// Maps file stem (without extension) → "container/module" ID
    file_stem_to_module: HashMap<String, String>,
}

impl ModuleIndex {
    fn build(
        _root: &Path,
        arch: &Architecture,
        details: &HashMap<String, ContainerDetail>,
    ) -> Self {
        let mut keyword_to_modules: HashMap<String, HashSet<String>> = HashMap::new();
        let mut file_stem_to_module: HashMap<String, String> = HashMap::new();

        for container in &arch.containers {
            let detail = match details.get(&container.id) {
                Some(d) => d,
                None => continue,
            };

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

                // Index by container.module pattern (e.g., "AITeam.Boards" → boards container)
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
        }
    }

    /// Resolve an import string to a set of possible module IDs.
    fn resolve(&self, import: &str, source_container: &str) -> HashSet<String> {
        let normalized = import.to_lowercase();
        let mut results = HashSet::new();

        // Strategy 1: Direct match on the full import (lowercased, separators removed)
        if let Some(modules) = self.keyword_to_modules.get(&normalized.replace('.', "").replace("::", "")) {
            results.extend(modules.iter().cloned());
        }

        // Strategy 2: For .NET imports (dot-separated), match the project prefix, not individual segments.
        // e.g., "AITeam.Boards.Schema" should match project "AITeam.Boards", not random "boards" segment.
        if normalized.contains('.') {
            // Try progressively shorter prefixes (longest match wins)
            let segments: Vec<&str> = normalized.split('.').collect();
            for len in (1..=segments.len()).rev() {
                let prefix = segments[..len].join(".");
                if let Some(modules) = self.keyword_to_modules.get(&prefix) {
                    results.extend(modules.iter().cloned());
                    break; // Longest match found
                }
            }
        } else if normalized.contains("::") {
            // Rust-style: match each segment (crate modules are single words)
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

        // Strategy 3: For Rust crate:: imports, match the path after crate::
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

        // Strategy 4: For Rust super:: imports, match the next segment
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

        // Filter out: self-references (same container), and standard library imports
        results.retain(|m| {
            let container = m.split('/').next().unwrap_or("");
            // Keep if it's a different container, or same container but different module
            container == source_container || !m.starts_with(source_container)
        });

        results
    }
}

/// Check if an import is to a standard library / external dependency (not project code).
fn is_external_import(import: &str) -> bool {
    let lower = import.to_lowercase();

    // .NET standard libraries
    if lower.starts_with("system") || lower.starts_with("microsoft.") || lower.starts_with("fsharp.") {
        return true;
    }

    // Rust standard library
    if lower.starts_with("std::") || lower.starts_with("core::") || lower.starts_with("alloc::") {
        return true;
    }

    // Python standard library (common ones)
    let python_std = [
        "os", "sys", "json", "io", "re", "math", "typing", "pathlib", "collections",
        "functools", "itertools", "datetime", "logging", "unittest", "abc", "enum",
        "dataclasses", "contextlib", "copy", "hashlib", "base64", "urllib", "http",
        "socket", "threading", "multiprocessing", "subprocess", "shutil", "glob",
        "tempfile", "argparse", "configparser", "csv", "xml", "html", "email",
    ];
    if python_std.contains(&lower.as_str()) {
        return true;
    }

    // Node.js built-ins
    let node_builtins = [
        "fs", "path", "os", "http", "https", "url", "crypto", "stream", "events",
        "child_process", "util", "assert", "buffer", "cluster", "dns", "net",
        "querystring", "readline", "tls", "zlib",
    ];
    if node_builtins.contains(&lower.as_str()) {
        return true;
    }

    // Go standard library (starts with no dots in first segment — heuristic)
    // External Go packages typically have dots (e.g., "github.com/...")
    // Standard Go packages are single words (e.g., "fmt", "os", "net/http")
    // This is a rough heuristic

    // Relative imports (Python/TS) — these are project-internal, not external
    if lower.starts_with('.') {
        return false;
    }

    // NPM packages — if it doesn't start with . or / it could be external
    // But we can't know for sure without package.json, so we don't filter these

    false
}

pub fn run() -> Result<(), String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let arch_path = crate::schema::find_arch_yaml()?;

    let content = std::fs::read_to_string(&arch_path).map_err(|e| e.to_string())?;
    let arch: Architecture =
        serde_yaml::from_str(&content).map_err(|e| format!("Invalid architecture.yaml: {e}"))?;

    // Load all container details
    let mut details: HashMap<String, ContainerDetail> = HashMap::new();
    for container in &arch.containers {
        let detail_path = root
            .join("architecture")
            .join(format!("{}.yaml", container.id));
        if detail_path.exists() {
            let detail_content =
                std::fs::read_to_string(&detail_path).map_err(|e| e.to_string())?;
            let detail: ContainerDetail = serde_yaml::from_str(&detail_content)
                .map_err(|e| format!("Invalid {}: {e}", detail_path.display()))?;
            details.insert(container.id.clone(), detail);
        }
    }

    // Build the module index
    let index = ModuleIndex::build(&root, &arch, &details);

    let mut drift_items: Vec<DriftItem> = Vec::new();
    let mut scanned_count = 0;

    for container in &arch.containers {
        let detail = match details.get(&container.id) {
            Some(d) => d,
            None => continue,
        };

        for module in &detail.modules {
            let file_path = root.join(&container.path).join(&module.file);
            if !file_path.exists() {
                continue;
            }

            let file_content = match std::fs::read_to_string(&file_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            scanned_count += 1;
            let file_imports = imports::extract_imports(&file_path, &file_content);

            // Build the set of declared dependencies for this module
            let declared_deps: HashSet<String> = module
                .depends_on
                .iter()
                .map(|d| d.to_lowercase())
                .collect();

            // Also consider container-level depends_on
            let container_deps: HashSet<String> = container
                .depends_on
                .iter()
                .map(|d| d.to_lowercase())
                .collect();

            let forbidden_deps: HashSet<String> = module
                .must_not_depend
                .iter()
                .map(|d| d.to_lowercase())
                .collect();

            let self_id = format!("{}/{}", container.id, module.id).to_lowercase();

            for imp in &file_imports {
                if is_external_import(&imp.raw) {
                    continue;
                }

                let resolved = index.resolve(&imp.raw, &container.id);
                if resolved.is_empty() {
                    continue; // Can't resolve → probably external dependency
                }

                for target in &resolved {
                    let target_lower = target.to_lowercase();

                    // Skip self-references
                    if target_lower == self_id {
                        continue;
                    }

                    // Skip same-container references (intra-container deps are fine
                    // unless explicitly forbidden)
                    let target_container = target_lower.split('/').next().unwrap_or("");
                    let is_same_container = target_container == container.id.to_lowercase();

                    // Check forbidden first
                    if forbidden_deps.contains(&target_lower) {
                        drift_items.push(DriftItem {
                            module_id: format!("{}/{}", container.id, module.id),
                            file: module.file.clone(),
                            import_raw: imp.raw.clone(),
                            line_number: imp.line_number,
                            target_module: target.clone(),
                            kind: DriftKind::Forbidden,
                        });
                        continue;
                    }

                    // Check undeclared (only for cross-container deps)
                    if !is_same_container && !declared_deps.contains(&target_lower) {
                        // Also check if just the container is declared (module-level)
                        let module_declared = declared_deps
                            .iter()
                            .any(|d| target_lower.starts_with(d.as_str()));
                        // Also check container-level depends_on
                        let container_level_declared = container_deps.contains(target_container);
                        if !module_declared && !container_level_declared {
                            drift_items.push(DriftItem {
                                module_id: format!("{}/{}", container.id, module.id),
                                file: module.file.clone(),
                                import_raw: imp.raw.clone(),
                                line_number: imp.line_number,
                                target_module: target.clone(),
                                kind: DriftKind::Undeclared,
                            });
                        }
                    }
                }
            }
        }
    }

    // Report
    if drift_items.is_empty() {
        println!(
            "✅ No dependency drift detected ({} modules scanned)",
            scanned_count
        );
    } else {
        let forbidden: Vec<&DriftItem> = drift_items
            .iter()
            .filter(|d| matches!(d.kind, DriftKind::Forbidden))
            .collect();
        let undeclared: Vec<&DriftItem> = drift_items
            .iter()
            .filter(|d| matches!(d.kind, DriftKind::Undeclared))
            .collect();

        if !forbidden.is_empty() {
            println!("🚫 {} forbidden dependency violation(s):\n", forbidden.len());
            for d in &forbidden {
                println!(
                    "  {} ({}:{}) → {} via `{}`",
                    d.module_id, d.file, d.line_number, d.target_module, d.import_raw
                );
            }
            println!();
        }

        if !undeclared.is_empty() {
            println!("⚠️  {} undeclared dependency(ies):\n", undeclared.len());
            for d in &undeclared {
                println!(
                    "  {} ({}:{}) → {} via `{}`",
                    d.module_id, d.file, d.line_number, d.target_module, d.import_raw
                );
            }
            println!();
        }

        println!(
            "📊 {} modules scanned, {} issue(s) found",
            scanned_count,
            drift_items.len()
        );

        if !forbidden.is_empty() {
            return Err(format!("{} forbidden violation(s)", forbidden.len()));
        }
    }

    Ok(())
}
