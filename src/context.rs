use crate::schema::{self, Architecture, ContainerDetail, Stories};
use crate::resolve::ModuleIndex;
use glob::Pattern;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Find architecture.yaml — checks architecture/architecture.yaml first, then root.
pub fn find_arch_yaml() -> Result<PathBuf, String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let nested = root.join("architecture").join("architecture.yaml");
    if nested.exists() {
        return Ok(nested);
    }
    let flat = root.join("architecture.yaml");
    if flat.exists() {
        return Ok(flat);
    }
    Err("architecture.yaml not found. Run `arch init` first.".into())
}

/// File extensions that have import parsers (used by drift, stories, fitness).
pub const SOURCE_EXTENSIONS: &[&str] = &[
    "rs", "fs", "fsx", "cs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt",
];

/// Superset of SOURCE_EXTENSIONS for coverage checks (includes languages without parsers).
pub const COVERAGE_EXTENSIONS: &[&str] = &[
    "rs", "fs", "fsx", "cs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt",
    "rb", "swift", "c", "cpp", "h", "hpp",
];

/// Loaded architecture context — the single entry point for all commands.
/// Loads root YAML, all container details, ignore patterns, and optionally
/// the module index and stories.
pub struct ArchContext {
    pub root: PathBuf,
    pub arch: Architecture,
    pub details: HashMap<String, ContainerDetail>,
    pub ignore_patterns: Vec<Pattern>,
}

impl ArchContext {
    /// Load architecture YAML and all container details.
    pub fn load() -> Result<Self, String> {
        let root = std::env::current_dir().map_err(|e| e.to_string())?;
        let arch_path = find_arch_yaml()?;

        let content = std::fs::read_to_string(&arch_path).map_err(|e| e.to_string())?;
        let arch: Architecture =
            serde_yaml::from_str(&content).map_err(|e| format!("Invalid architecture.yaml: {e}"))?;

        let ignore_patterns = schema::compile_ignore_patterns(&arch.system.ignore);

        let mut details: HashMap<String, ContainerDetail> = HashMap::new();
        for container in &arch.containers {
            let detail_path = root
                .join("architecture")
                .join(format!("{}.yaml", container.id));
            if detail_path.exists() {
                let detail_content =
                    std::fs::read_to_string(&detail_path).map_err(|e| e.to_string())?;
                let detail: ContainerDetail = serde_yaml::from_str(&detail_content)
                    .map_err(|e| format!("Invalid {}: {e}", detail_path.display()))?;
                details.insert(container.id.clone(), detail);
            }
        }

        Ok(ArchContext { root, arch, details, ignore_patterns })
    }

    /// Build the module resolution index from loaded data.
    pub fn build_index(&self) -> ModuleIndex {
        ModuleIndex::build(&self.root, &self.arch, &self.details)
    }

    /// Load stories from architecture/stories.yaml.
    pub fn load_stories(&self) -> Result<Option<Stories>, String> {
        let stories_path = self.root.join("architecture").join("stories.yaml");
        if !stories_path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&stories_path).map_err(|e| e.to_string())?;
        let stories: Stories = serde_yaml::from_str(&content)
            .map_err(|e| format!("Invalid stories.yaml: {e}"))?;
        Ok(Some(stories))
    }

    /// Collect the canonical paths of all explicitly mapped module files.
    pub fn collect_mapped_files(&self) -> HashSet<PathBuf> {
        let mut mapped = HashSet::new();
        for container in &self.arch.containers {
            if let Some(detail) = self.details.get(&container.id) {
                for module in &detail.modules {
                    for file in module.all_files() {
                        let full_path = self.root.join(&container.path).join(file);
                        if let Ok(canonical) = full_path.canonicalize() {
                            mapped.insert(canonical);
                        }
                    }
                }
            }
        }
        mapped
    }
}

/// Walk source files for a module, expanding directory-owner files into all
/// source files in their directory tree. Skips files already mapped to other modules.
pub fn walk_module_files(
    root: &Path,
    container_path: &str,
    module: &schema::Module,
    explicitly_mapped: &HashSet<PathBuf>,
    extensions: &HashSet<&str>,
    ignore_patterns: &[Pattern],
) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let skip_dirs: HashSet<&str> = schema::SKIP_DIRS.iter().copied().collect();

    for file in module.all_files() {
        let entry_path = root.join(container_path).join(file);
        if !entry_path.exists() {
            continue;
        }

        let rel_path = format!("{}/{}", container_path, file);
        if schema::is_ignored(&rel_path, ignore_patterns) {
            continue;
        }

        files.push(entry_path.clone());

        // Directory-owner files: scan all source files in directory tree
        if schema::is_directory_owner(file) {
            if let Some(dir) = entry_path.parent() {
                for entry in WalkDir::new(dir)
                    .into_iter()
                    .filter_entry(|e| {
                        !e.file_type().is_dir()
                            || !skip_dirs.contains(e.file_name().to_str().unwrap_or(""))
                    })
                    .filter_map(|e| e.ok())
                    .filter(|e| e.file_type().is_file())
                {
                    let path = entry.path().to_path_buf();
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                    if !extensions.contains(ext) {
                        continue;
                    }
                    if let Ok(canonical) = path.canonicalize() {
                        if explicitly_mapped.contains(&canonical) {
                            continue;
                        }
                    }
                    files.push(path);
                }
            }
        }
    }
    files
}

/// Serialize a value to pretty JSON. Returns Err on serialization failure
/// instead of panicking.
pub fn to_json(value: &serde_json::Value) -> Result<String, String> {
    serde_json::to_string_pretty(value).map_err(|e| format!("JSON serialization failed: {e}"))
}

/// Print a JSON value to stdout. Convenience wrapper around to_json.
pub fn print_json(value: &serde_json::Value) -> Result<(), String> {
    println!("{}", to_json(value)?);
    Ok(())
}
