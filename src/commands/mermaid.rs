use crate::context::ArchContext;
use std::fmt::Write;
use std::path::Path;
use std::process::Command;

pub fn run(
    stories_mode: bool,
    brief: bool,
    c4: bool,
    svg: Option<String>,
    update_readme: bool,
) -> Result<(), String> {
    let ctx = ArchContext::load()?;

    let mermaid = if stories_mode {
        render_stories(&ctx)?
    } else if c4 {
        render_c4(&ctx)?
    } else {
        render_containers(&ctx, brief)?
    };

    if let Some(path) = svg {
        export_svg(&mermaid, &path)?;
        eprintln!("✅ SVG written to {path}");
    } else if update_readme {
        inject_readme(&mermaid)?;
        eprintln!("✅ README.md updated");
    } else {
        print!("{mermaid}");
    }

    Ok(())
}

fn render_containers(ctx: &ArchContext, brief: bool) -> Result<String, String> {
    let mut out = String::new();
    writeln!(out, "graph LR").unwrap();

    for container in &ctx.arch.containers {
        let module_count = ctx
            .details
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
        writeln!(out, "    {node_id}[\"{label}\"]").unwrap();
    }

    writeln!(out).unwrap();

    for container in &ctx.arch.containers {
        let from = sanitize_id(&container.id);
        for dep in &container.depends_on {
            let to = sanitize_id(dep);
            writeln!(out, "    {from} --> {to}").unwrap();
        }
    }

    if !ctx.details.is_empty() {
        writeln!(out).unwrap();
        for container in &ctx.arch.containers {
            if let Some(detail) = ctx.details.get(&container.id) {
                if detail.modules.len() > 1 {
                    let sub_id = sanitize_id(&container.id);
                    let label = if brief {
                        &container.id
                    } else {
                        container.description.as_deref().unwrap_or(&container.id)
                    };
                    writeln!(out, "    subgraph {sub_id}_detail[\"{label}\"]").unwrap();
                    for module in &detail.modules {
                        let mod_id =
                            sanitize_id(&format!("{}_{}", container.id, module.id));
                        writeln!(out, "        {mod_id}[\"{}\"]", module.id).unwrap();
                    }
                    for module in &detail.modules {
                        let from_id =
                            sanitize_id(&format!("{}_{}", container.id, module.id));
                        for dep in &module.depends_on {
                            if !dep.contains('/') {
                                let to_id =
                                    sanitize_id(&format!("{}_{}", container.id, dep));
                                writeln!(out, "        {from_id} --> {to_id}").unwrap();
                            }
                        }
                    }
                    writeln!(out, "    end").unwrap();
                }
            }
        }
    }

    Ok(out)
}

fn render_c4(ctx: &ArchContext) -> Result<String, String> {
    let mut out = String::new();
    let system_name = &ctx.arch.system.name;

    writeln!(out, "C4Container").unwrap();
    writeln!(out, "    title Container diagram for {system_name}").unwrap();
    writeln!(out).unwrap();

    writeln!(out, "    System_Boundary(system, \"{system_name}\") {{").unwrap();
    for container in &ctx.arch.containers {
        let id = sanitize_id(&container.id);
        let name = &container.id;
        let desc = container.description.as_deref().unwrap_or("");
        let tech = container.project.as_deref().unwrap_or("");
        writeln!(
            out,
            "        Container({id}, \"{name}\", \"{tech}\", \"{desc}\")"
        )
        .unwrap();
    }
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    for container in &ctx.arch.containers {
        let from = sanitize_id(&container.id);
        for dep in &container.depends_on {
            let to = sanitize_id(dep);
            writeln!(out, "    Rel({from}, {to}, \"depends on\")").unwrap();
        }
    }

    Ok(out)
}

fn render_stories(ctx: &ArchContext) -> Result<String, String> {
    let stories = match ctx.load_stories()? {
        Some(s) => s,
        None => return Err("No stories.yaml found in architecture/".to_string()),
    };

    if stories.stories.is_empty() {
        return Ok("No stories defined\n".to_string());
    }

    let mut out = String::new();
    for (i, story) in stories.stories.iter().enumerate() {
        if i > 0 {
            writeln!(out).unwrap();
        }
        let desc = story.description.trim().replace('\n', " ");
        writeln!(out, "---").unwrap();
        writeln!(out, "title: {} — {}", story.id, desc).unwrap();
        writeln!(out, "---").unwrap();
        writeln!(out, "flowchart LR").unwrap();

        for (j, step) in story.flow.iter().enumerate() {
            let node_id = format!("s{}_{}", i, j);
            let label = step.split('/').last().unwrap_or(step);
            writeln!(out, "    {node_id}[\"{label}\"]").unwrap();
        }

        for j in 0..story.flow.len().saturating_sub(1) {
            let from = format!("s{}_{}", i, j);
            let to = format!("s{}_{}", i, j + 1);
            writeln!(out, "    {from} --> {to}").unwrap();
        }
    }

    Ok(out)
}

fn export_svg(mermaid: &str, path: &str) -> Result<(), String> {
    let temp_path = format!("{path}.mmd");
    std::fs::write(&temp_path, mermaid)
        .map_err(|e| format!("Failed to write temp file: {e}"))?;

    let result = Command::new("mmdc")
        .args(["-i", &temp_path, "-o", path, "-b", "transparent"])
        .output()
        .map_err(|e| {
            let _ = std::fs::remove_file(&temp_path);
            format!(
                "Failed to run mmdc (install with: npm i -g @mermaid-js/mermaid-cli): {e}"
            )
        })?;

    let _ = std::fs::remove_file(&temp_path);

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(format!("mmdc failed: {stderr}"));
    }

    Ok(())
}

fn inject_readme(mermaid: &str) -> Result<(), String> {
    let readme_path = Path::new("README.md");
    if !readme_path.exists() {
        return Err("README.md not found in current directory".to_string());
    }

    let content =
        std::fs::read_to_string(readme_path).map_err(|e| format!("Failed to read README.md: {e}"))?;

    let start_marker = "<!-- arch:mermaid:start -->";
    let end_marker = "<!-- arch:mermaid:end -->";

    // Find markers that are NOT inside fenced code blocks
    let start_pos = find_outside_code_blocks(&content, start_marker);
    let end_pos = find_outside_code_blocks(&content, end_marker);

    let block = format!("{start_marker}\n```mermaid\n{mermaid}```\n{end_marker}");

    let new_content = match (start_pos, end_pos) {
        (Some(start), Some(end)) => {
            let end = end + end_marker.len();
            format!("{}{}{}", &content[..start], block, &content[end..])
        }
        (Some(_), None) => {
            return Err(format!(
                "Found {start_marker} but no matching {end_marker}"
            ));
        }
        _ => {
            return Err(format!(
                "No markers found. Add these to your README.md where you want the diagram:\n\n\
                 {start_marker}\n{end_marker}"
            ));
        }
    };

    std::fs::write(readme_path, new_content)
        .map_err(|e| format!("Failed to write README.md: {e}"))?;

    Ok(())
}

/// Find a marker string in content, skipping occurrences inside fenced code blocks.
fn find_outside_code_blocks(content: &str, marker: &str) -> Option<usize> {
    let mut in_code_block = false;
    let mut offset = 0;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
        }
        if !in_code_block {
            if let Some(pos) = line.find(marker) {
                return Some(offset + pos);
            }
        }
        // Advance by the actual byte length of this line in the original content,
        // accounting for both LF and CRLF line endings. content.lines() strips \r\n
        // to give the logical line, so line.len() never includes \r. Using find('\n')
        // on the original slice gives the correct advance for both formats.
        let advance = content[offset..]
            .find('\n')
            .map(|n| n + 1)             // skip past '\n'; '\r' (if any) is included in n
            .unwrap_or(content[offset..].len()); // last line, no trailing newline
        offset += advance;
    }
    None
}

fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_outside_code_blocks_lf() {
        let content = "line one\n<!-- marker -->\nline three\n";
        let pos = find_outside_code_blocks(content, "<!-- marker -->");
        assert_eq!(pos, Some(9), "LF: marker should be at byte 9");
    }

    #[test]
    fn test_find_outside_code_blocks_crlf() {
        // CRLF content: each newline is \r\n (2 bytes instead of 1)
        let content = "line one\r\n<!-- marker -->\r\nline three\r\n";
        let pos = find_outside_code_blocks(content, "<!-- marker -->");
        assert_eq!(pos, Some(10), "CRLF: marker should be at byte 10 (after 'line one\\r\\n')");
    }

    #[test]
    fn test_find_outside_code_blocks_skips_code_fence() {
        let content = "```\n<!-- marker -->\n```\n<!-- marker -->\n";
        let pos = find_outside_code_blocks(content, "<!-- marker -->");
        // "```\n" = 4 bytes, "<!-- marker -->\n" = 16 bytes, "```\n" = 4 bytes → second marker at 24
        assert_eq!(pos, Some(24), "Should skip marker inside code block");
    }
}
