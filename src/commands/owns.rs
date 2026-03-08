use crate::schema::{Architecture, ContainerDetail};

pub fn run(concept: &str) -> Result<(), String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let arch_path = root.join("architecture.yaml");

    if !arch_path.exists() {
        return Err("architecture.yaml not found. Run `arch init` first.".into());
    }

    let content = std::fs::read_to_string(&arch_path).map_err(|e| e.to_string())?;
    let arch: Architecture =
        serde_yaml::from_str(&content).map_err(|e| format!("Invalid architecture.yaml: {e}"))?;

    let query = concept.to_lowercase();
    let mut matches = Vec::new();

    for container in &arch.containers {
        let detail_path = root
            .join("architecture")
            .join(format!("{}.yaml", container.id));
        if !detail_path.exists() {
            continue;
        }

        let detail_content = std::fs::read_to_string(&detail_path).map_err(|e| e.to_string())?;
        let detail: ContainerDetail = serde_yaml::from_str(&detail_content)
            .map_err(|e| format!("Invalid {}: {e}", detail_path.display()))?;

        for module in &detail.modules {
            // Search owns fields
            for owned in &module.owns {
                if owned.to_lowercase().contains(&query) {
                    matches.push((
                        format!("{}/{}", container.id, module.id),
                        module.file.clone(),
                        owned.clone(),
                        module.boundary.clone(),
                    ));
                }
            }
            // Also match on module id or file name
            if module.id.to_lowercase().contains(&query)
                || module.file.to_lowercase().contains(&query)
            {
                let already = matches
                    .iter()
                    .any(|(p, _, _, _)| *p == format!("{}/{}", container.id, module.id));
                if !already {
                    matches.push((
                        format!("{}/{}", container.id, module.id),
                        module.file.clone(),
                        format!("[module: {}]", module.id),
                        module.boundary.clone(),
                    ));
                }
            }
        }
    }

    if matches.is_empty() {
        println!("No module owns a concept matching '{concept}'");
    } else {
        for (module_path, file, owned_concept, boundary) in &matches {
            println!("📍 {module_path}");
            println!("   file: {file}");
            println!("   owns: {owned_concept}");
            if let Some(b) = boundary {
                println!("   boundary: {b}");
            }
            println!();
        }
    }

    Ok(())
}
