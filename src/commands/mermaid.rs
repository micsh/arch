use crate::context::ArchContext;

pub fn run(stories_mode: bool, brief: bool) -> Result<(), String> {
    let ctx = ArchContext::load()?;

    if stories_mode {
        render_stories(&ctx)
    } else {
        render_containers(&ctx, brief)
    }
}

fn render_containers(ctx: &ArchContext, brief: bool) -> Result<(), String> {
    println!("graph LR");

    for container in &ctx.arch.containers {
        let module_count = ctx.details
            .get(&container.id)
            .map(|d| d.modules.len())
            .unwrap_or(0);
        let node_id = sanitize_id(&container.id);
        let label = if brief {
            container.id.clone()
        } else {
            let desc = container.description.as_deref().unwrap_or(&container.id);
            if module_count > 0 {
                format!("{desc}\\n({module_count} modules)")
            } else {
                desc.to_string()
            }
        };
        println!("    {node_id}[\"{label}\"]");
    }

    println!();

    for container in &ctx.arch.containers {
        let from = sanitize_id(&container.id);
        for dep in &container.depends_on {
            let to = sanitize_id(dep);
            println!("    {from} --> {to}");
        }
    }

    if !ctx.details.is_empty() {
        println!();
        for container in &ctx.arch.containers {
            if let Some(detail) = ctx.details.get(&container.id) {
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
                        println!("        {mod_id}[\"{}\"]", module.id);
                    }
                    for module in &detail.modules {
                        let from_id = sanitize_id(&format!("{}_{}", container.id, module.id));
                        for dep in &module.depends_on {
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

fn render_stories(ctx: &ArchContext) -> Result<(), String> {
    let stories = match ctx.load_stories()? {
        Some(s) => s,
        None => return Err("No stories.yaml found in architecture/".to_string()),
    };

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
            let label = step.split('/').last().unwrap_or(step);
            println!("    {node_id}[\"{label}\"]");
        }

        for j in 0..story.flow.len().saturating_sub(1) {
            let from = format!("s{}_{}", i, j);
            let to = format!("s{}_{}", i, j + 1);
            println!("    {from} --> {to}");
        }
    }

    Ok(())
}

fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}
