use crate::schema::{Architecture, ContainerDetail};
use std::path::Path;

pub fn run() -> Result<(), String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let arch_path = root.join("architecture.yaml");

    if !arch_path.exists() {
        return Err("architecture.yaml not found. Run `arch init` first.".into());
    }

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

    // Report
    if errors.is_empty() && warnings.is_empty() {
        println!("✅ Architecture is valid ({} containers)", arch.containers.len());
    } else {
        for w in &warnings {
            println!("⚠️  {w}");
        }
        for e in &errors {
            println!("❌ {e}");
        }
        if !errors.is_empty() {
            return Err(format!("{} error(s), {} warning(s)", errors.len(), warnings.len()));
        }
        println!(
            "\n✅ Valid with {} warning(s)",
            warnings.len()
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
        // Check file exists
        let file_path = container_root.join(&module.file);
        if !file_path.exists() {
            errors.push(format!(
                "{}/{}: file '{}' does not exist",
                container.id, module.id, module.file
            ));
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
