use crate::imports;
use crate::resolve::{is_external_import, ModuleIndex};
use crate::schema::{self, Architecture, ContainerDetail};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

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

pub fn run() -> Result<(), String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let arch_path = crate::schema::find_arch_yaml()?;

    let content = std::fs::read_to_string(&arch_path).map_err(|e| e.to_string())?;
    let arch: Architecture =
        serde_yaml::from_str(&content).map_err(|e| format!("Invalid architecture.yaml: {e}"))?;

    let ignore_patterns = schema::compile_ignore_patterns(&arch.system.ignore);

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

    // Pre-build set of all explicitly mapped file paths (canonical)
    // so entry-point directory scanning skips files owned by other modules
    let mut explicitly_mapped: HashSet<PathBuf> = HashSet::new();
    for container in &arch.containers {
        if let Some(detail) = details.get(&container.id) {
            for module in &detail.modules {
                let full_path = root.join(&container.path).join(&module.file);
                if let Ok(canonical) = full_path.canonicalize() {
                    explicitly_mapped.insert(canonical);
                }
            }
        }
    }

    let source_extensions: HashSet<&str> = [
        "rs", "fs", "fsx", "cs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt",
    ]
    .into();

    for container in &arch.containers {
        let detail = match details.get(&container.id) {
            Some(d) => d,
            None => continue,
        };

        for module in &detail.modules {
            let entry_path = root.join(&container.path).join(&module.file);
            if !entry_path.exists() {
                continue;
            }

            // Skip ignored files
            let rel_path = format!("{}/{}", container.path, module.file);
            if schema::is_ignored(&rel_path, &ignore_patterns) {
                continue;
            }

            // Collect files to scan: the entry point + sibling files for entry-point modules
            let mut files_to_scan: Vec<PathBuf> = vec![entry_path.clone()];

            if schema::is_entry_point(&module.file) {
                if let Some(dir) = entry_path.parent() {
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if !path.is_file() {
                                continue;
                            }
                            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                            if !source_extensions.contains(ext) {
                                continue;
                            }
                            // Skip files explicitly mapped to another module
                            if let Ok(canonical) = path.canonicalize() {
                                if explicitly_mapped.contains(&canonical) {
                                    continue;
                                }
                            }
                            files_to_scan.push(path);
                        }
                    }
                }
            }

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

            for scan_path in &files_to_scan {
                let file_content = match std::fs::read_to_string(scan_path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                scanned_count += 1;
                let file_imports = imports::extract_imports(scan_path, &file_content);

                // Display name: relative to container path
                let display_file = scan_path
                    .strip_prefix(root.join(&container.path))
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| module.file.clone());

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
                                file: display_file.clone(),
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
                                    file: display_file.clone(),
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
