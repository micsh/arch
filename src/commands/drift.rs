use crate::context::{self, ArchContext, print_json};
use crate::depgraph;
use crate::imports;
use crate::resolve::is_external_import;
use crate::schema;
use serde::Serialize;
use std::collections::HashSet;

#[derive(Serialize)]
pub struct DriftItem {
    pub module_id: String,
    pub file: String,
    pub import_raw: String,
    pub line_number: usize,
    pub target_module: String,
    pub kind: String,
}

#[derive(Serialize)]
pub struct StaleDeclaredItem {
    pub module_id: String,
    pub file: String,
    pub declared_dep: String,
}

pub struct DriftResult {
    pub items: Vec<DriftItem>,
    pub scanned_count: usize,
    pub stale_declared: Vec<StaleDeclaredItem>,
}

/// Scan source files and return all drift violations without printing anything.
pub fn check(ctx: &ArchContext) -> Result<DriftResult, String> {
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

                    let leading = imp.raw.split(['.', ':']).next().unwrap_or("").to_lowercase();
                    let leading_norm = leading.replace('-', "").replace('_', "");
                    if !leading_norm.is_empty() {
                        let is_self_ref = module.owns.iter().any(|o| {
                            let own_norm = o.replace('-', "").replace('_', "").to_lowercase();
                            own_norm == leading_norm
                        }) || {
                            let mod_norm = module.id.replace('-', "").replace('_', "").to_lowercase();
                            mod_norm == leading_norm
                        };
                        if is_self_ref {
                            continue;
                        }
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

    Ok(DriftResult { items: drift_items, scanned_count, stale_declared: Vec::new() })
}

pub fn run(json: bool, stale_declared: bool) -> Result<(), String> {
    let ctx = ArchContext::load()?;
    let mut result = check(&ctx)?;

    if stale_declared {
        result.stale_declared = check_stale_declared(&ctx);
    }

    report_drift(json, stale_declared, result.scanned_count, &result.items, &result.stale_declared)
}

/// Compute (declared − actual): deps declared in .arch but never imported in practice.
///
/// ASSUMPTION: container.depends_on stale deps are not checked — module-level only.
/// IF INVALID: add a second pass over container.depends_on × actual container-level dep graph.
pub fn check_stale_declared(ctx: &ArchContext) -> Vec<StaleDeclaredItem> {
    use std::collections::HashMap;

    let index = ctx.build_index();
    // resolve_self=true so same-container refs are included in actual graph
    let actual_deps: HashMap<String, HashSet<String>> = depgraph::build_dep_graph(ctx, &index, true);

    let mut stale = Vec::new();

    for container in &ctx.arch.containers {
        let detail = match ctx.details.get(&container.id) {
            Some(d) => d,
            None => continue,
        };

        for module in &detail.modules {
            let module_id = format!("{}/{}", container.id, module.id).to_lowercase();
            let actual = actual_deps.get(&module_id).cloned().unwrap_or_default();

            for dep in &module.depends_on {
                let dep_lower = dep.to_lowercase();
                // Check if any actual dep starts with or equals the declared dep
                let is_used = actual.iter().any(|a| {
                    a == &dep_lower || a.starts_with(&format!("{}/", dep_lower))
                });
                if !is_used {
                    let file = module.file.clone();
                    stale.push(StaleDeclaredItem {
                        module_id: format!("{}/{}", container.id, module.id),
                        file,
                        declared_dep: dep.clone(),
                    });
                }
            }
        }
    }

    stale
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
    use std::collections::HashMap;

    let source_id = format!("{}/{}", container_id, module.id);

    // Group cross-container targets by their container
    let mut by_container: HashMap<String, Vec<String>> = HashMap::new();
    for target in resolved {
        let target_lower = target.to_lowercase();
        if target_lower == self_id {
            continue;
        }
        let target_container = target_lower.split('/').next().unwrap_or("").to_string();
        if target_container == container_id.to_lowercase() {
            continue; // same container — not a drift violation
        }
        by_container.entry(target_container).or_default().push(target.clone());
    }

    for (target_container, targets) in &by_container {
        // Broad fan-out: multiple targets in one foreign container from a single import
        // → collapse to single container-level violation with [broad match] tag
        if targets.len() > 1 {
            let any_forbidden = forbidden_deps.contains(target_container)
                || targets.iter().any(|t| forbidden_deps.contains(&t.to_lowercase()));

            if any_forbidden {
                drift_items.push(DriftItem {
                    module_id: source_id.clone(),
                    file: display_file.to_string(),
                    import_raw: imp.raw.clone(),
                    line_number: imp.line_number,
                    target_module: format!("{} [broad match]", target_container),
                    kind: "forbidden".to_string(),
                });
            } else {
                let module_declared = declared_deps.iter()
                    .any(|d| d.starts_with(&format!("{}/", target_container)));
                let container_level_declared = container_deps.contains(target_container.as_str());
                if !module_declared && !container_level_declared {
                    drift_items.push(DriftItem {
                        module_id: source_id.clone(),
                        file: display_file.to_string(),
                        import_raw: imp.raw.clone(),
                        line_number: imp.line_number,
                        target_module: format!("{} [broad match]", target_container),
                        kind: "undeclared".to_string(),
                    });
                }
            }
            continue;
        }

        // Single target in this foreign container — check individually
        let target = &targets[0];
        let target_lower = target.to_lowercase();

        if forbidden_deps.contains(&target_lower) {
            drift_items.push(DriftItem {
                module_id: source_id.clone(),
                file: display_file.to_string(),
                import_raw: imp.raw.clone(),
                line_number: imp.line_number,
                target_module: target.clone(),
                kind: "forbidden".to_string(),
            });
            continue;
        }

        if !declared_deps.contains(&target_lower) {
            let module_declared = declared_deps
                .iter()
                .any(|d| target_lower.starts_with(d.as_str()));
            let container_level_declared = container_deps.contains(target_container.as_str());
            if !module_declared && !container_level_declared {
                drift_items.push(DriftItem {
                    module_id: source_id.clone(),
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

fn report_drift(json: bool, stale_declared: bool, scanned_count: usize, drift_items: &[DriftItem], stale: &[StaleDeclaredItem]) -> Result<(), String> {
    if json {
        let output = serde_json::json!({
            "scanned": scanned_count,
            "issues": drift_items.len(),
            "forbidden": drift_items.iter().filter(|d| d.kind == "forbidden").collect::<Vec<_>>(),
            "undeclared": drift_items.iter().filter(|d| d.kind == "undeclared").collect::<Vec<_>>(),
            "stale_declared": stale,
        });
        print_json(&output)?;
        let forbidden_count = drift_items.iter().filter(|d| d.kind == "forbidden").count();
        if forbidden_count > 0 {
            return Err(format!("{forbidden_count} forbidden violation(s)"));
        }
        return Ok(());
    }

    if drift_items.is_empty() && (!stale_declared || stale.is_empty()) {
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

        if stale_declared && !stale.is_empty() {
            println!("\n⚠️  {} stale declared dep(s):\n", stale.len());
            for s in stale {
                println!("  {} ({}): declares dep on {} but no imports resolve to it", s.module_id, s.file, s.declared_dep);
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
