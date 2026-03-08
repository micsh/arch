use crate::schema::{Architecture, ContainerDetail};
use std::collections::HashSet;
use walkdir::WalkDir;

pub fn run() -> Result<(), String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let arch_path = root.join("architecture.yaml");

    if !arch_path.exists() {
        return Err("architecture.yaml not found. Run `arch init` first.".into());
    }

    let content = std::fs::read_to_string(&arch_path).map_err(|e| e.to_string())?;
    let arch: Architecture =
        serde_yaml::from_str(&content).map_err(|e| format!("Invalid architecture.yaml: {e}"))?;

    // Collect all mapped files
    let mut mapped_files = HashSet::new();

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
            let full_path = root.join(&container.path).join(&module.file);
            if let Ok(canonical) = full_path.canonicalize() {
                mapped_files.insert(canonical);
            }
        }
    }

    // Walk source directories and find unmapped files
    let source_extensions: HashSet<&str> = [
        "rs", "fs", "fsx", "cs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt", "rb",
        "swift", "c", "cpp", "h", "hpp",
    ]
    .into();

    let mut unmapped = Vec::new();

    for container in &arch.containers {
        let container_path = root.join(&container.path);
        if !container_path.exists() {
            continue;
        }

        for entry in WalkDir::new(&container_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");

            if source_extensions.contains(ext) {
                if let Ok(canonical) = path.canonicalize() {
                    if !mapped_files.contains(&canonical) {
                        let relative = path.strip_prefix(&root).unwrap_or(path);
                        unmapped.push(relative.display().to_string());
                    }
                }
            }
        }
    }

    if unmapped.is_empty() {
        println!("✅ All source files are mapped to modules");
    } else {
        println!("📂 {} unmapped source file(s):\n", unmapped.len());
        for f in &unmapped {
            println!("  {f}");
        }
    }

    Ok(())
}
