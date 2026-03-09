use crate::schema::{Architecture, ContainerDetail};
use std::path::Path;

pub struct ValidationResult {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub container_count: usize,
}

/// Core validation logic — returns structured results without printing.
pub fn check() -> Result<ValidationResult, String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let arch_path = crate::schema::find_arch_yaml()?;

    let content = std::fs::read_to_string(&arch_path).map_err(|e| e.to_string())?;
    let arch: Architecture =
        serde_yaml::from_str(&content).map_err(|e| format!("Invalid architecture.yaml: {e}"))?;

    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Check system fields
    if arch.system.name.is_empty() {
        errors.push("system.name is empty".to_string());
    }

    // Check containers
    let container_ids: Vec<&str> = arch.containers.iter().map(|c| c.id.as_str()).collect();

    for container in &arch.containers {
        // Check path exists
        if !root.join(&container.path).exists() {
            errors.push(format!(
                "Container '{}': path '{}' does not exist",
                container.id, container.path
            ));
        }

        // Check depends_on references valid containers
        for dep in &container.depends_on {
            if !container_ids.contains(&dep.as_str()) {
                errors.push(format!(
                    "Container '{}': depends_on '{}' not found in containers",
                    container.id, dep
                ));
            }
        }

        // Check container detail file exists
        let detail_path = root
            .join("architecture")
            .join(format!("{}.yaml", container.id));
        if !detail_path.exists() {
            warnings.push(format!(
                "Container '{}': no detail file at architecture/{}.yaml",
                container.id, container.id
            ));
        } else {
            validate_container_detail(&root, container, &detail_path, &mut errors)?;
        }
    }

    // Check for orphan container YAML files
    let arch_dir = root.join("architecture");
    if arch_dir.is_dir() {
        let known_ids: std::collections::HashSet<String> =
            arch.containers.iter().map(|c| c.id.clone()).collect();
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
        container_count: arch.containers.len(),
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
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
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
) -> Result<(), String> {
    let content = std::fs::read_to_string(detail_path).map_err(|e| e.to_string())?;
    let detail: ContainerDetail =
        serde_yaml::from_str(&content).map_err(|e| format!("Invalid {}: {e}", detail_path.display()))?;

    let container_root = root.join(&container.path);

    for module in &detail.modules {
        // Check all files exist
        for file in module.all_files() {
            let file_path = container_root.join(file);
            if !file_path.exists() {
                errors.push(format!(
                    "{}/{}: file '{}' does not exist",
                    container.id, module.id, file
                ));
            }
        }

        // Check owns is not empty
        if module.owns.is_empty() {
            errors.push(format!(
                "{}/{}: 'owns' is empty — every module should own at least one concept",
                container.id, module.id
            ));
        }
    }

    Ok(())
}
