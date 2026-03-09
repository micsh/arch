use crate::schema::{Architecture, ContainerDetail, Stories};
use std::collections::HashMap;

pub fn run(stories_mode: bool, brief: bool) -> Result<(), String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let arch_path = crate::schema::find_arch_yaml()?;

    let content = std::fs::read_to_string(&arch_path).map_err(|e| e.to_string())?;
    let arch: Architecture =
        serde_yaml::from_str(&content).map_err(|e| format!("Invalid architecture.yaml: {e}"))?;

    if stories_mode {
        render_stories(&root, &arch)
    } else {
        render_containers(&root, &arch, brief)
    }
}

/// Render a container-level dependency diagram.
fn render_containers(
    root: &std::path::Path,
    arch: &Architecture,
    brief: bool,
) -> Result<(), String> {
    // Load container details for module counts
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

    println!("graph LR");

    // Emit container nodes with descriptions
    for container in &arch.containers {
        let module_count = details
            .get(&container.id)
            .map(|d| d.modules.len())
            .unwrap_or(0);
        let node_id = sanitize_id(&container.id);
        let label = if brief {
            container.id.clone()
        } else {
            let desc = container
                .description
                .as_deref()
                .unwrap_or(&container.id);
            if module_count > 0 {
                format!("{desc}\\n({module_count} modules)")
            } else {
                desc.to_string()
            }
        };
        println!("    {node_id}[\"{label}\"]");
    }

    println!();

    // Emit dependency edges
    for container in &arch.containers {
        let from = sanitize_id(&container.id);
        for dep in &container.depends_on {
            let to = sanitize_id(dep);
            println!("    {from} --> {to}");
        }
    }

    // If there are details, also emit module-level subgraphs
    if !details.is_empty() {
        println!();
        for container in &arch.containers {
            if let Some(detail) = details.get(&container.id) {
                if detail.modules.len() > 1 {
                    let sub_id = sanitize_id(&container.id);
                    let label = if brief {
                        &container.id
                    } else {
                        container.description.as_deref().unwrap_or(&container.id)
                    };
                    println!("    subgraph {sub_id}_detail[\"{label}\"]");
                    for module in &detail.modules {
                        let mod_id = sanitize_id(&format!("{}_{}", container.id, module.id));
                        let mod_label = &module.id;
                        println!("        {mod_id}[\"{mod_label}\"]");
                    }
                    // Emit intra-container dependencies
                    for module in &detail.modules {
                        let from_id = sanitize_id(&format!("{}_{}", container.id, module.id));
                        for dep in &module.depends_on {
                            // Only intra-container deps (no slash = same container)
                            if !dep.contains('/') {
                                let to_id = sanitize_id(&format!("{}_{}", container.id, dep));
                                println!("        {from_id} --> {to_id}");
                            }
                        }
                    }
                    println!("    end");
                }
            }
        }
    }

    Ok(())
}

/// Render story flow diagrams.
fn render_stories(
    root: &std::path::Path,
    _arch: &Architecture,
) -> Result<(), String> {
    let stories_path = root.join("architecture").join("stories.yaml");
    if !stories_path.exists() {
        return Err("No stories.yaml found in architecture/".to_string());
    }
    let stories_content = std::fs::read_to_string(&stories_path).map_err(|e| e.to_string())?;
    let stories: Stories = serde_yaml::from_str(&stories_content)
        .map_err(|e| format!("Invalid stories.yaml: {e}"))?;

    if stories.stories.is_empty() {
        println!("No stories defined");
        return Ok(());
    }

    for (i, story) in stories.stories.iter().enumerate() {
        if i > 0 {
            println!();
        }
        let desc = story.description.trim().replace('\n', " ");
        println!("---");
        println!("title: {} — {}", story.id, desc);
        println!("---");
        println!("flowchart LR");

        for (j, step) in story.flow.iter().enumerate() {
            let node_id = format!("s{}_{}", i, j);
            // Use the part after '/' as short label, full ID as tooltip
            let label = step.split('/').last().unwrap_or(step);
            println!("    {node_id}[\"{label}\"]");
        }

        // Chain arrows
        for j in 0..story.flow.len().saturating_sub(1) {
            let from = format!("s{}_{}", i, j);
            let to = format!("s{}_{}", i, j + 1);
            println!("    {from} --> {to}");
        }
    }

    Ok(())
}

/// Sanitize a string for use as a Mermaid node ID (replace non-alphanumeric with _).
fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}
