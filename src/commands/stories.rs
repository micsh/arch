use crate::imports;
use crate::resolve::{is_external_import, ModuleIndex};
use crate::schema::{self, Architecture, ContainerDetail, Stories};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Build the actual dependency graph from source files.
/// Returns: module_id (lowercase) → set of module_ids it imports from.
fn build_dep_graph(
    root: &std::path::Path,
    arch: &Architecture,
    details: &HashMap<String, ContainerDetail>,
    index: &ModuleIndex,
) -> HashMap<String, HashSet<String>> {
    let source_extensions: HashSet<&str> = [
        "rs", "fs", "fsx", "cs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt",
    ]
    .into();

    // Pre-build explicitly mapped files so entry-point scanning skips them
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

    let mut actual_deps: HashMap<String, HashSet<String>> = HashMap::new();

    for container in &arch.containers {
        let detail = match details.get(&container.id) {
            Some(d) => d,
            None => continue,
        };

        for module in &detail.modules {
            let full_id = format!("{}/{}", container.id, module.id).to_lowercase();
            let entry_path = root.join(&container.path).join(&module.file);

            let mut files_to_scan: Vec<PathBuf> = vec![];
            if entry_path.exists() {
                files_to_scan.push(entry_path.clone());
            }

            // Entry-point modules: also scan sibling source files in the directory
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

            let mut deps = HashSet::new();
            for scan_path in &files_to_scan {
                let file_content = match std::fs::read_to_string(scan_path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let file_imports = imports::extract_imports(scan_path, &file_content);
                for imp in &file_imports {
                    if is_external_import(&imp.raw) {
                        continue;
                    }
                    let resolved = index.resolve(&imp.raw, &container.id);
                    for target in &resolved {
                        let target_lower = target.to_lowercase();
                        if target_lower != full_id {
                            deps.insert(target_lower);
                        }
                    }
                }
            }
            actual_deps.insert(full_id, deps);
        }
    }

    actual_deps
}

pub fn run() -> Result<(), String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let arch_path = schema::find_arch_yaml()?;

    let content = std::fs::read_to_string(&arch_path).map_err(|e| e.to_string())?;
    let arch: Architecture =
        serde_yaml::from_str(&content).map_err(|e| format!("Invalid architecture.yaml: {e}"))?;

    // Load stories
    let stories_path = root.join("architecture").join("stories.yaml");
    if !stories_path.exists() {
        println!("📖 No stories.yaml found in architecture/");
        return Ok(());
    }
    let stories_content = std::fs::read_to_string(&stories_path).map_err(|e| e.to_string())?;
    let stories: Stories = serde_yaml::from_str(&stories_content)
        .map_err(|e| format!("Invalid stories.yaml: {e}"))?;

    if stories.stories.is_empty() {
        println!("📖 No stories defined");
        return Ok(());
    }

    // Load container details
    let mut details: HashMap<String, ContainerDetail> = HashMap::new();
    for container in &arch.containers {
        let detail_path = root
            .join("architecture")
            .join(format!("{}.yaml", container.id));
        if detail_path.exists() {
            let dc = std::fs::read_to_string(&detail_path).map_err(|e| e.to_string())?;
            let detail: ContainerDetail = serde_yaml::from_str(&dc)
                .map_err(|e| format!("Invalid {}: {e}", detail_path.display()))?;
            details.insert(container.id.clone(), detail);
        }
    }

    // Build module index and dependency graph
    let index = ModuleIndex::build(&root, &arch, &details);
    let actual_deps = build_dep_graph(&root, &arch, &details, &index);

    // Build lookup sets
    let container_ids: HashSet<String> = arch
        .containers
        .iter()
        .map(|c| c.id.to_lowercase())
        .collect();

    let all_module_ids: HashSet<String> = actual_deps.keys().cloned().collect();

    // Validate each story
    let mut total_stories = 0;
    let mut passed_stories = 0;

    for story in &stories.stories {
        total_stories += 1;
        let desc = story.description.trim().replace('\n', " ");
        let desc_display = if desc.len() > 80 {
            format!("{}…", &desc[..77])
        } else {
            desc
        };
        println!("📖 {} — {}", story.id, desc_display);

        if story.flow.len() < 2 {
            println!("  ⚠️  Flow has fewer than 2 steps\n");
            continue;
        }

        // Validate all flow steps reference known modules or containers
        let mut all_valid = true;
        for step in &story.flow {
            let lower = step.to_lowercase();
            if !all_module_ids.contains(&lower) && !container_ids.contains(&lower) {
                println!("  ❌ Unknown: {step}");
                all_valid = false;
            }
        }
        if !all_valid {
            println!();
            continue;
        }

        // Check consecutive pairs for import connections
        let mut connections = 0;
        let mut gaps = 0;
        let mut skips = 0;

        for pair in story.flow.windows(2) {
            let from = &pair[0];
            let to = &pair[1];
            let from_lower = from.to_lowercase();
            let to_lower = to.to_lowercase();

            // Skip if either is a container-only reference (no module to verify)
            if !from_lower.contains('/') || !to_lower.contains('/') {
                skips += 1;
                println!("  {} → {}  ⏭️  container ref", from, to);
                continue;
            }

            // Bidirectional check: stories describe runtime flow, which may be
            // inverse of import direction (event-driven, callback patterns).
            // A→B in the flow means "A and B are connected", check both directions.
            let has_dep = |from_id: &str, to_id: &str| -> bool {
                actual_deps
                    .get(from_id)
                    .map(|deps| {
                        deps.contains(to_id)
                            || deps.iter().any(|d| d.starts_with(&format!("{to_id}/")))
                    })
                    .unwrap_or(false)
            };

            let forward = has_dep(&from_lower, &to_lower);
            let reverse = has_dep(&to_lower, &from_lower);

            // Same-container modules in compiled languages (.NET) share an assembly
            // and can reference each other without explicit import statements
            let from_container = from_lower.split('/').next().unwrap_or("");
            let to_container = to_lower.split('/').next().unwrap_or("");
            let same_container = from_container == to_container;

            if forward || reverse {
                connections += 1;
                println!("  {} → {}  ✅", from, to);
            } else if same_container {
                connections += 1;
                println!("  {} → {}  ✅ same project", from, to);
            } else {
                gaps += 1;
                println!("  {} → {}  ❌ no import found", from, to);
            }
        }

        let verified = connections + gaps;
        if gaps == 0 {
            passed_stories += 1;
            if skips > 0 {
                println!(
                    "  ✅ {connections}/{verified} verified, {skips} skipped\n"
                );
            } else {
                println!("  ✅ {connections}/{verified} connections verified\n");
            }
        } else {
            println!(
                "  ⚠️  {connections}/{verified} verified, {gaps} gap(s)\n"
            );
        }
    }

    println!("📊 {passed_stories}/{total_stories} stories fully connected");

    if passed_stories < total_stories {
        Err(format!(
            "{} story issue(s)",
            total_stories - passed_stories
        ))
    } else {
        Ok(())
    }
}
