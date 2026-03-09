use crate::schema::{Architecture, ContainerDetail};

pub fn run(concept: &str, json: bool) -> Result<(), String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let arch_path = crate::schema::find_arch_yaml()?;

    let content = std::fs::read_to_string(&arch_path).map_err(|e| e.to_string())?;
    let arch: Architecture =
        serde_yaml::from_str(&content).map_err(|e| format!("Invalid architecture.yaml: {e}"))?;

    let query = concept.to_lowercase();
    let mut matches: Vec<serde_json::Value> = Vec::new();

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
                    matches.push(serde_json::json!({
                        "module": format!("{}/{}", container.id, module.id),
                        "file": module.file,
                        "owns": owned,
                        "boundary": module.boundary,
                    }));
                }
            }
            // Also match on module id or file name
            if module.id.to_lowercase().contains(&query)
                || module.file.to_lowercase().contains(&query)
            {
                let module_path = format!("{}/{}", container.id, module.id);
                let already = matches.iter().any(|m| m["module"] == module_path);
                if !already {
                    matches.push(serde_json::json!({
                        "module": module_path,
                        "file": module.file,
                        "owns": format!("[module: {}]", module.id),
                        "boundary": module.boundary,
                    }));
                }
            }
        }
    }

    if json {
        let output = serde_json::json!({
            "query": concept,
            "matches": matches,
        });
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
        return Ok(());
    }

    if matches.is_empty() {
        println!("No module owns a concept matching '{concept}'");
    } else {
        for m in &matches {
            let module_path = m["module"].as_str().unwrap_or("");
            let file = m["file"].as_str().unwrap_or("");
            let owned = m["owns"].as_str().unwrap_or("");
            let boundary = m["boundary"].as_str();
            println!("📍 {module_path}");
            println!("   file: {file}");
            println!("   owns: {owned}");
            if let Some(b) = boundary {
                println!("   boundary: {b}");
            }
            println!();
        }
    }

    Ok(())
}
