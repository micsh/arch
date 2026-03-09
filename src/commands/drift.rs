use crate::context::{self, ArchContext, print_json};
use crate::imports;
use crate::resolve::is_external_import;
use crate::schema;
use serde::Serialize;
use std::collections::HashSet;

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
    let ctx = ArchContext::load()?;
    let index = ctx.build_index();
    let explicitly_mapped = ctx.collect_mapped_files();
    let source_ext: HashSet<&str> = context::SOURCE_EXTENSIONS.iter().copied().collect();

    let mut drift_items: Vec<DriftItem> = Vec::new();
    let mut scanned_count = 0;

    for container in &ctx.arch.containers {
        let detail = match ctx.details.get(&container.id) {
            Some(d) => d,
            None => continue,
        };

        for module in &detail.modules {
            let files_to_scan = context::walk_module_files(
                &ctx.root, &container.path, module,
                &explicitly_mapped, &source_ext, &ctx.ignore_patterns,
            );

            let declared_deps: HashSet<String> = module
                .depends_on.iter().map(|d| d.to_lowercase()).collect();
            let container_deps: HashSet<String> = container
                .depends_on.iter().map(|d| d.to_lowercase()).collect();
            let forbidden_deps: HashSet<String> = module
                .must_not_depend.iter().map(|d| d.to_lowercase()).collect();
            let self_id = format!("{}/{}", container.id, module.id).to_lowercase();

            for scan_path in &files_to_scan {
                let file_content = match std::fs::read_to_string(scan_path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                scanned_count += 1;
                let file_imports = imports::extract_imports(scan_path, &file_content);
                let display_file = scan_path
                    .strip_prefix(ctx.root.join(&container.path))
                    .map(|p| p.display().to_string().replace('\\', "/"))
                    .unwrap_or_else(|_| module.file.clone());

                for imp in &file_imports {
                    if is_external_import(&imp.raw) {
                        continue;
                    }

                    let resolved = index.resolve(&imp.raw, &container.id);
                    if resolved.is_empty() {
                        continue;
                    }

                    check_drift(
                        &resolved, &self_id, &container.id, &declared_deps,
                        &container_deps, &forbidden_deps, module, &display_file,
                        imp, &mut drift_items,
                    );
                }
            }
        }
    }

    report_drift(json, scanned_count, &drift_items)
}

fn check_drift(
    resolved: &HashSet<String>,
    self_id: &str,
    container_id: &str,
    declared_deps: &HashSet<String>,
    container_deps: &HashSet<String>,
    forbidden_deps: &HashSet<String>,
    module: &schema::Module,
    display_file: &str,
    imp: &imports::Import,
    drift_items: &mut Vec<DriftItem>,
) {
    // Detect broad fan-out: all resolved targets in same foreign container
    // → collapse to single container-level violation
    let cross_container: Vec<&String> = resolved.iter()
        .filter(|t| {
            let tl = t.to_lowercase();
            tl != self_id && t.split('/').next().unwrap_or("") != container_id
        })
        .collect();

    if cross_container.len() > 1 {
        let containers: HashSet<&str> = cross_container.iter()
            .map(|t| t.split('/').next().unwrap_or(""))
            .collect();
        if containers.len() == 1 {
            // All targets in one foreign container — this is broad fan-out
            let target_container = *containers.iter().next().unwrap();
            let target_lower = target_container.to_lowercase();

            if forbidden_deps.iter().any(|d| d == &target_lower || cross_container.iter().any(|t| t.to_lowercase() == *d)) {
                drift_items.push(DriftItem {
                    module_id: format!("{}/{}", container_id, module.id),
                    file: display_file.to_string(),
                    import_raw: imp.raw.clone(),
                    line_number: imp.line_number,
                    target_module: format!("{} [broad match]", target_container),
                    kind: "forbidden".to_string(),
                });
            } else if !declared_deps.iter().any(|d| target_lower.starts_with(d.as_str()))
                && !container_deps.contains(target_container)
            {
                drift_items.push(DriftItem {
                    module_id: format!("{}/{}", container_id, module.id),
                    file: display_file.to_string(),
                    import_raw: imp.raw.clone(),
                    line_number: imp.line_number,
                    target_module: format!("{} [broad match]", target_container),
                    kind: "undeclared".to_string(),
                });
            }
            return;
        }
    }

    // Normal path: check each target individually
    for target in resolved {
        let target_lower = target.to_lowercase();
        if target_lower == self_id {
            continue;
        }

        let target_container = target_lower.split('/').next().unwrap_or("");
        let is_same_container = target_container == container_id.to_lowercase();

        if forbidden_deps.contains(&target_lower) {
            drift_items.push(DriftItem {
                module_id: format!("{}/{}", container_id, module.id),
                file: display_file.to_string(),
                import_raw: imp.raw.clone(),
                line_number: imp.line_number,
                target_module: target.clone(),
                kind: "forbidden".to_string(),
            });
            continue;
        }

        if !is_same_container && !declared_deps.contains(&target_lower) {
            let module_declared = declared_deps
                .iter()
                .any(|d| target_lower.starts_with(d.as_str()));
            let container_level_declared = container_deps.contains(target_container);
            if !module_declared && !container_level_declared {
                drift_items.push(DriftItem {
                    module_id: format!("{}/{}", container_id, module.id),
                    file: display_file.to_string(),
                    import_raw: imp.raw.clone(),
                    line_number: imp.line_number,
                    target_module: target.clone(),
                    kind: "undeclared".to_string(),
                });
            }
        }
    }
}

fn report_drift(json: bool, scanned_count: usize, drift_items: &[DriftItem]) -> Result<(), String> {
    if json {
        let output = serde_json::json!({
            "scanned": scanned_count,
            "issues": drift_items.len(),
            "forbidden": drift_items.iter().filter(|d| d.kind == "forbidden").collect::<Vec<_>>(),
            "undeclared": drift_items.iter().filter(|d| d.kind == "undeclared").collect::<Vec<_>>(),
        });
        print_json(&output)?;
        let forbidden_count = drift_items.iter().filter(|d| d.kind == "forbidden").count();
        if forbidden_count > 0 {
            return Err(format!("{forbidden_count} forbidden violation(s)"));
        }
        return Ok(());
    }

    if drift_items.is_empty() {
        println!("✅ No dependency drift detected ({scanned_count} modules scanned)");
    } else {
        let forbidden: Vec<&DriftItem> = drift_items.iter().filter(|d| d.kind == "forbidden").collect();
        let undeclared: Vec<&DriftItem> = drift_items.iter().filter(|d| d.kind == "undeclared").collect();

        if !forbidden.is_empty() {
            println!("🚫 {} forbidden dependency violation(s):\n", forbidden.len());
            for d in &forbidden {
                println!("  {} ({}:{})", d.module_id, d.file, d.line_number);
                println!("    import: {} → {}", d.import_raw, d.target_module);
            }
            println!();
        }
        if !undeclared.is_empty() {
            println!("⚠️  {} undeclared dependency(ies):\n", undeclared.len());
            for d in &undeclared {
                println!("  {} ({}:{})", d.module_id, d.file, d.line_number);
                println!("    import: {} → {}", d.import_raw, d.target_module);
            }
        }

        let total = drift_items.len();
        println!("\n📊 {total} issue(s) found ({scanned_count} modules scanned)");
        if !forbidden.is_empty() {
            return Err(format!("{} forbidden violation(s)", forbidden.len()));
        }
    }
    Ok(())
}
