use crate::context::{self, ArchContext, print_json};
use crate::schema;
use std::collections::HashSet;
use std::path::PathBuf;
use walkdir::WalkDir;

pub struct CoverageResult {
    pub unmapped: Vec<String>,
}

/// Core coverage logic — returns structured results without printing.
pub fn check(ctx: &ArchContext) -> Result<CoverageResult, String> {
    // Collect all mapped files and directories covered by entry-point modules
    let mut mapped_files: HashSet<PathBuf> = HashSet::new();
    let mut covered_dirs: HashSet<PathBuf> = HashSet::new();

    for container in &ctx.arch.containers {
        if let Some(detail) = ctx.details.get(&container.id) {
            for module in &detail.modules {
                for file in module.all_files() {
                    let full_path = ctx.root.join(&container.path).join(file);
                    if let Ok(canonical) = full_path.canonicalize() {
                        mapped_files.insert(canonical);
                    }

                    if schema::is_directory_owner(file) {
                        if let Some(parent) = full_path.parent() {
                            if let Ok(canonical_dir) = parent.canonicalize() {
                                covered_dirs.insert(canonical_dir);
                            }
                        }
                    }
                }
            }
        }
    }

    // Coverage uses a broader set of extensions than import-scanning commands
    let coverage_ext: HashSet<&str> = context::COVERAGE_EXTENSIONS.iter().copied().collect();

    let mut unmapped = Vec::new();
    let skip_dirs: HashSet<&str> = schema::SKIP_DIRS.iter().copied().collect();

    for container in &ctx.arch.containers {
        let container_path = ctx.root.join(&container.path);
        if !container_path.exists() {
            continue;
        }

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
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

            if coverage_ext.contains(ext) {
                if let Ok(canonical) = path.canonicalize() {
                    if !mapped_files.contains(&canonical) {
                        let in_covered_dir = is_in_covered_dir(path, &covered_dirs);
                        if !in_covered_dir {
                            let relative = path.strip_prefix(&ctx.root).unwrap_or(path);
                            let rel_str = relative.display().to_string();
                            if !schema::is_ignored(&rel_str, &ctx.ignore_patterns) {
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

/// Check if a file path is in a directory (or subdirectory) covered by a directory-owner module.
fn is_in_covered_dir(path: &std::path::Path, covered_dirs: &HashSet<PathBuf>) -> bool {
    path.parent()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| {
            let mut dir = p.as_path();
            loop {
                if covered_dirs.contains(dir) {
                    return true;
                }
                match dir.parent() {
                    Some(parent) if parent != dir => dir = parent,
                    _ => return false,
                }
            }
        })
        .unwrap_or(false)
}

pub fn run(json: bool) -> Result<(), String> {
    let ctx = ArchContext::load()?;
    let result = check(&ctx)?;

    if json {
        let output = serde_json::json!({
            "unmapped": result.unmapped,
            "count": result.unmapped.len(),
        });
        print_json(&output)?;
    } else if result.unmapped.is_empty() {
        println!("✅ All source files are mapped to modules");
    } else {
        println!("📂 {} unmapped source file(s):\n", result.unmapped.len());
        for f in &result.unmapped {
            println!("  {f}");
        }
    }

    Ok(())
}
