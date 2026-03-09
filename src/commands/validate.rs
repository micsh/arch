use crate::context::{ArchContext, print_json};
use crate::schema::ContainerDetail;
use std::path::Path;

pub struct ValidationResult {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub container_count: usize,
}

/// Core validation logic — returns structured results without printing.
pub fn check() -> Result<ValidationResult, String> {
    let ctx = ArchContext::load()?;
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if ctx.arch.system.name.is_empty() {
        errors.push("system.name is empty".to_string());
    }

    let container_ids: Vec<&str> = ctx.arch.containers.iter().map(|c| c.id.as_str()).collect();

    // Build set of all "container/module" IDs for cross-reference validation
    let mut all_module_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for container in &ctx.arch.containers {
        if let Some(detail) = ctx.details.get(&container.id) {
            for module in &detail.modules {
                all_module_ids.insert(format!("{}/{}", container.id, module.id));
            }
        }
    }

    for container in &ctx.arch.containers {
        if !ctx.root.join(&container.path).exists() {
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

        let detail_path = ctx.root
            .join("architecture")
            .join(format!("{}.yaml", container.id));
        if !detail_path.exists() {
            warnings.push(format!(
                "Container '{}': no detail file at architecture/{}.yaml",
                container.id, container.id
            ));
        } else {
            validate_container_detail(&ctx.root, container, &detail_path, &mut errors, &mut warnings, &all_module_ids)?;
        }
    }

    // Check for orphan container YAML files
    let arch_dir = ctx.root.join("architecture");
    if arch_dir.is_dir() {
        let known_ids: std::collections::HashSet<String> =
            ctx.arch.containers.iter().map(|c| c.id.clone()).collect();
        let special_files = ["architecture.yaml", "stories.yaml"];
        if let Ok(entries) = std::fs::read_dir(&arch_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".yaml") || name.ends_with(".yml") {
                    if special_files.contains(&name.as_str()) {
                        continue;
                    }
                    let id = name.trim_end_matches(".yaml").trim_end_matches(".yml");
                    if !known_ids.contains(id) {
                        warnings.push(format!(
                            "Orphan file 'architecture/{name}' — no container with id '{id}' in architecture.yaml"
                        ));
                    }
                }
            }
        }
    }

    Ok(ValidationResult {
        errors,
        warnings,
        container_count: ctx.arch.containers.len(),
    })
}

pub fn run(json: bool) -> Result<(), String> {
    let result = check()?;

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
    detail_path: &Path,
    errors: &mut Vec<String>,
    warnings: &mut Vec<String>,
    all_module_ids: &std::collections::HashSet<String>,
) -> Result<(), String> {
    let content = std::fs::read_to_string(detail_path).map_err(|e| e.to_string())?;
    let detail: ContainerDetail =
        serde_yaml::from_str(&content).map_err(|e| format!("Invalid {}: {e}", detail_path.display()))?;

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
