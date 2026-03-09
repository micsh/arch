use crate::context::{ArchContext, print_json};

pub fn run(concept: &str, json: bool) -> Result<(), String> {
    let ctx = ArchContext::load()?;
    let query = concept.to_lowercase();
    let mut matches: Vec<serde_json::Value> = Vec::new();

    for container in &ctx.arch.containers {
        let detail = match ctx.details.get(&container.id) {
            Some(d) => d,
            None => continue,
        };

        for module in &detail.modules {
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
        print_json(&output)?;
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
