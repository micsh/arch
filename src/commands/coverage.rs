use crate::schema::{self, Architecture, ContainerDetail};
use std::collections::HashSet;
use std::path::PathBuf;
use walkdir::WalkDir;

pub struct CoverageResult {
    pub unmapped: Vec<String>,
}

/// Core coverage logic — returns structured results without printing.
pub fn check() -> Result<CoverageResult, String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let arch_path = crate::schema::find_arch_yaml()?;

    let content = std::fs::read_to_string(&arch_path).map_err(|e| e.to_string())?;
    let arch: Architecture =
        serde_yaml::from_str(&content).map_err(|e| format!("Invalid architecture.yaml: {e}"))?;

    let ignore_patterns = schema::compile_ignore_patterns(&arch.system.ignore);

    // Collect all mapped files and directories covered by entry-point modules
    let mut mapped_files: HashSet<PathBuf> = HashSet::new();
    let mut covered_dirs: HashSet<PathBuf> = HashSet::new();

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

            // If this module's file is an entry point, mark its directory as covered
            if schema::is_entry_point(&module.file) {
                if let Some(parent) = full_path.parent() {
                    if let Ok(canonical_dir) = parent.canonicalize() {
                        covered_dirs.insert(canonical_dir);
                    }
                }
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

        let skip_dirs: HashSet<&str> =
            ["obj", "bin", "target", "node_modules", ".git", "dist", "build", "__pycache__"].into();

        for entry in WalkDir::new(&container_path)
            .into_iter()
            .filter_entry(|e| {
                !e.file_type().is_dir()
                    || !skip_dirs.contains(e.file_name().to_str().unwrap_or(""))
            })
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
                        // Check if file is in a directory covered by an entry-point module
                        let in_covered_dir = path
                            .parent()
                            .and_then(|p| p.canonicalize().ok())
                            .map(|p| covered_dirs.contains(&p))
                            .unwrap_or(false);

                        if !in_covered_dir {
                            let relative = path.strip_prefix(&root).unwrap_or(path);
                            let rel_str = relative.display().to_string();
                            if !schema::is_ignored(&rel_str, &ignore_patterns) {
                                unmapped.push(rel_str);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(CoverageResult { unmapped })
}

pub fn run() -> Result<(), String> {
    let result = check()?;

    if result.unmapped.is_empty() {
        println!("✅ All source files are mapped to modules");
    } else {
        println!("📂 {} unmapped source file(s):\n", result.unmapped.len());
        for f in &result.unmapped {
            println!("  {f}");
        }
    }

    Ok(())
}
