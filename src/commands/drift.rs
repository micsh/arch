use crate::imports;
use crate::resolve::{is_external_import, ModuleIndex};
use crate::schema::{self, Architecture, ContainerDetail};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[derive(Serialize)]
struct DriftItem {
    module_id: String,
    file: String,
    import_raw: String,
    line_number: usize,
    target_module: String,
    kind: String,
}

pub fn run(json: bool) -> Result<(), String> {
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
                for file in module.all_files() {
                    let full_path = root.join(&container.path).join(file);
                    if let Ok(canonical) = full_path.canonicalize() {
                        explicitly_mapped.insert(canonical);
                    }
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
            // Collect files to scan from all module files
            let mut files_to_scan: Vec<PathBuf> = Vec::new();

            for file in module.all_files() {
                let entry_path = root.join(&container.path).join(file);
                if !entry_path.exists() {
                    continue;
                }

                // Skip ignored files
                let rel_path = format!("{}/{}", container.path, file);
                if schema::is_ignored(&rel_path, &ignore_patterns) {
                    continue;
                }

                files_to_scan.push(entry_path.clone());

                if schema::is_directory_owner(file) {
                    if let Some(dir) = entry_path.parent() {
                    let skip_dirs: HashSet<&str> = crate::schema::SKIP_DIRS.iter().copied().collect();
                    for entry in walkdir::WalkDir::new(dir)
                        .into_iter()
                        .filter_entry(|e| {
                            !e.file_type().is_dir()
                                || !skip_dirs.contains(e.file_name().to_str().unwrap_or(""))
                        })
                        .filter_map(|e| e.ok())
                        .filter(|e| e.file_type().is_file())
                    {
                        let path = entry.path().to_path_buf();
                        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                        if !source_extensions.contains(ext) {
                            continue;
                        }
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

                // Display name: relative to container path, normalized to forward slashes
                let display_file = scan_path
                    .strip_prefix(root.join(&container.path))
                    .map(|p| p.display().to_string().replace('\\', "/"))
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
                                kind: "forbidden".to_string(),
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
                                    kind: "undeclared".to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Report
    if json {
        let output = serde_json::json!({
            "scanned": scanned_count,
            "issues": drift_items.len(),
            "forbidden": drift_items.iter().filter(|d| d.kind == "forbidden").collect::<Vec<_>>(),
            "undeclared": drift_items.iter().filter(|d| d.kind == "undeclared").collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
        let forbidden_count = drift_items.iter().filter(|d| d.kind == "forbidden").count();
        if forbidden_count > 0 {
            return Err(format!("{forbidden_count} forbidden violation(s)"));
        }
        return Ok(());
    }

    if drift_items.is_empty() {
        println!(
            "✅ No dependency drift detected ({} modules scanned)",
            scanned_count
        );
    } else {
        let forbidden: Vec<&DriftItem> = drift_items
            .iter()
            .filter(|d| d.kind == "forbidden")
            .collect();
        let undeclared: Vec<&DriftItem> = drift_items
            .iter()
            .filter(|d| d.kind == "undeclared")
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
