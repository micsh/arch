use crate::context::{ArchContext, print_json};
use crate::llmcode;
use crate::schema::ContainerDetail;
use std::path::Path;

pub struct ValidationResult {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub container_count: usize,
}

/// Core validation logic — returns structured results without printing.
pub fn check(ctx: &ArchContext) -> Result<ValidationResult, String> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if ctx.arch.system.name.is_empty() {
        errors.push("system.name is empty".to_string());
    }

    let container_ids: Vec<&str> = ctx.arch.containers.iter().map(|c| c.id.as_str()).collect();

    // Build set of all "container/module" IDs — used for cross-reference and MOD: link validation
    let all_module_ids: std::collections::HashSet<String> = ctx.arch.containers
        .iter()
        .flat_map(|c| {
            ctx.details.get(&c.id).into_iter().flat_map(move |d| {
                d.modules.iter().map(move |m| format!("{}/{}", c.id, m.id))
            })
        })
        .collect();

    for container in &ctx.arch.containers {
        if !container.path.is_empty() && !ctx.root.join(&container.path).exists() {
            errors.push(format!(
                "Container '{}': path '{}' does not exist",
                container.id, container.path
            ));
        }

        for dep in &container.depends_on {
            if !container_ids.contains(&dep.as_str()) {
                errors.push(format!(
                    "Container '{}': depends_on '{}' not found in containers",
                    container.id, dep
                ));
            }
        }

        match ctx.details.get(&container.id) {
            None => errors.push(format!(
                "Container '{}': no module definition found (missing architecture/arch/containers/{}.arch)",
                container.id, container.id
            )),
            Some(detail) => {
                validate_container_detail(&ctx.root, container, detail, &mut errors, &mut warnings, &all_module_ids)?;
            }
        }
    }

    // Check for orphan container .arch files (stem not matching any declared container)
    let containers_dir = ctx.root.join("architecture").join("arch").join("containers");
    if containers_dir.is_dir() {
        let known_ids: std::collections::HashSet<String> =
            ctx.arch.containers.iter().map(|c| c.id.clone()).collect();
        if let Ok(entries) = std::fs::read_dir(&containers_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".arch") {
                    let id = name.trim_end_matches(".arch");
                    if !known_ids.contains(id) {
                        warnings.push(format!(
                            "Orphan file 'architecture/arch/containers/{name}' — no container with id '{id}' declared in system.arch"
                        ));
                    }
                }
            }
        }
    }

    // Validate MOD: links and CNTR: module references in .llmcode files
    let llmcode_files_list = llmcode::discover_llmcode_files(&ctx.root);
    for path in &llmcode_files_list {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                warnings.push(format!(
                    "llmcode: could not read '{}': {e}",
                    path.display()
                ));
                continue;
            }
        };
        match llmcode::parse_llmcode_file(path, &content) {
            Ok(parsed) => {
                // MOD: link validation → hard errors
                for err in llmcode::validate_mod_links(&[parsed], &all_module_ids) {
                    errors.push(format!("{err}"));
                }
                // CNTR: module reference validation → warnings only (symbols change; stale refs expected)
                // Re-parse for CNTR entries (parsed was moved into validate_mod_links)
                if let Ok(reparsed) = llmcode::parse_llmcode_file(path, &content) {
                    for block in &reparsed.blocks {
                        for cntr in &block.cntr {
                            validate_cntr_side(&cntr.left_module, &cntr.constraint, path, &all_module_ids, &mut warnings);
                            validate_cntr_side(&cntr.right_module, &cntr.constraint, path, &all_module_ids, &mut warnings);
                        }
                    }
                }
            }
            Err(e) => warnings.push(format!("llmcode: parse error in '{}': {e}", path.display())),
        }
    }

    Ok(ValidationResult {
        errors,
        warnings,
        container_count: ctx.arch.containers.len(),
    })
}

/// Check one side of a CNTR: entry for a stale module reference.
/// CNTR: sides use "module::symbol" format — only validates the module prefix if it contains '/'.
fn validate_cntr_side(
    side: &str,
    constraint: &str,
    path: &std::path::Path,
    valid_ids: &std::collections::HashSet<String>,
    warnings: &mut Vec<String>,
) {
    let module_part = side.split("::").next().unwrap_or(side);
    if module_part.contains('/') && !valid_ids.contains(module_part) {
        warnings.push(format!(
            "llmcode CNTR: '{}' — '{}' not found in arch modules (stale reference, constraint: {})",
            path.display(), module_part, constraint
        ));
    }
}

pub fn run(json: bool) -> Result<(), String> {
    let ctx = ArchContext::load()?;
    let result = check(&ctx)?;

    if json {
        let output = serde_json::json!({
            "valid": result.errors.is_empty(),
            "containers": result.container_count,
            "errors": result.errors,
            "warnings": result.warnings,
        });
        print_json(&output)?;
        if !result.errors.is_empty() {
            return Err(format!("{} error(s)", result.errors.len()));
        }
        return Ok(());
    }

    if result.errors.is_empty() && result.warnings.is_empty() {
        println!("✅ Architecture is valid ({} containers)", result.container_count);
    } else {
        for w in &result.warnings {
            println!("⚠️  {w}");
        }
        for e in &result.errors {
            println!("❌ {e}");
        }
        if !result.errors.is_empty() {
            return Err(format!("{} error(s), {} warning(s)", result.errors.len(), result.warnings.len()));
        }
        println!(
            "\n✅ Valid with {} warning(s)",
            result.warnings.len()
        );
    }

    Ok(())
}

fn validate_container_detail(
    root: &Path,
    container: &crate::schema::Container,
    detail: &ContainerDetail,
    errors: &mut Vec<String>,
    warnings: &mut Vec<String>,
    all_module_ids: &std::collections::HashSet<String>,
) -> Result<(), String> {
    let container_root = root.join(&container.path);

    // Check for duplicate module IDs within this container
    let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for module in &detail.modules {
        if !seen_ids.insert(module.id.clone()) {
            errors.push(format!(
                "{}/{}: duplicate module id '{}' — IDs must be unique within a container",
                container.id, module.id, module.id
            ));
        }
    }

    for module in &detail.modules {
        for file in module.all_files() {
            let file_path = container_root.join(file);
            if !file_path.exists() {
                errors.push(format!(
                    "{}/{}: file '{}' does not exist",
                    container.id, module.id, file
                ));
            }
        }

        if module.owns.is_empty() {
            errors.push(format!(
                "{}/{}: 'owns' is empty — every module should own at least one concept",
                container.id, module.id
            ));
        }

        // Warn on unresolved depends_on references
        for dep in &module.depends_on {
            if !dep.contains('/') && !all_module_ids.contains(dep) {
                // Bare name — check if it's a known module ID in this container
                let local = format!("{}/{}", container.id, dep);
                if !all_module_ids.contains(&local) {
                    warnings.push(format!(
                        "{}/{}: depends_on '{}' — bare name, should use container/module format",
                        container.id, module.id, dep
                    ));
                }
            } else if dep.contains('/') && !all_module_ids.contains(dep) {
                warnings.push(format!(
                    "{}/{}: depends_on '{}' — module not found",
                    container.id, module.id, dep
                ));
            }
        }
    }

    Ok(())
}
