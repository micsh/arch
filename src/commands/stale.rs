use crate::schema::{Architecture, ContainerDetail};
use std::process::Command;

pub fn run() -> Result<(), String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let arch_path = root.join("architecture.yaml");

    if !arch_path.exists() {
        return Err("architecture.yaml not found. Run `arch init` first.".into());
    }

    let content = std::fs::read_to_string(&arch_path).map_err(|e| e.to_string())?;
    let arch: Architecture =
        serde_yaml::from_str(&content).map_err(|e| format!("Invalid architecture.yaml: {e}"))?;

    // Get YAML last-modified time
    let yaml_mtime = get_git_last_modified("architecture.yaml")?;

    let mut stale_modules = Vec::new();

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

        let container_yaml_mtime = get_git_last_modified(&format!("architecture/{}.yaml", container.id))
            .unwrap_or(yaml_mtime);

        for module in &detail.modules {
            let source_path = format!("{}/{}", container.path, module.file);
            if let Ok(source_mtime) = get_git_last_modified(&source_path) {
                if source_mtime > container_yaml_mtime {
                    stale_modules.push((
                        format!("{}/{}", container.id, module.id),
                        source_path,
                    ));
                }
            }
        }
    }

    if stale_modules.is_empty() {
        println!("✅ All architecture definitions are up to date");
    } else {
        println!(
            "⏰ {} module(s) may have stale architecture definitions:\n",
            stale_modules.len()
        );
        for (module, file) in &stale_modules {
            println!("  {module}  ({file} changed after YAML)");
        }
    }

    Ok(())
}

fn get_git_last_modified(path: &str) -> Result<i64, String> {
    let output = Command::new("git")
        .args(["log", "-1", "--format=%ct", "--", path])
        .output()
        .map_err(|e| format!("Failed to run git: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let timestamp = stdout
        .trim()
        .parse::<i64>()
        .map_err(|_| format!("No git history for {path}"))?;

    Ok(timestamp)
}
