use crate::arch_parser;
use crate::schema::{
    self, Architecture, ArchSource, ArchSourceContainer, ArchSourceModule,
    Container, ContainerDetail, Module, Rule, Stories, Story, System,
};
use crate::resolve::ModuleIndex;
use glob::Pattern;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Find architecture/arch/system.arch from cwd.
///
/// ASSUMPTION: system.arch always lives at architecture/arch/system.arch relative to the
/// project root (cwd). IF INVALID (monorepo root vs sub-project): accept root as parameter.
pub fn find_system_arch() -> Result<PathBuf, String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let path = root.join("architecture").join("arch").join("system.arch");
    if path.exists() {
        return Ok(path);
    }
    Err("architecture/arch/system.arch not found. Run `arch init` first.".into())
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
/// Populated from `.arch` source files via arch_parser; no YAML dependency.
pub struct ArchContext {
    pub root: PathBuf,
    pub arch: Architecture,
    pub details: HashMap<String, ContainerDetail>,
    pub ignore_patterns: Vec<Pattern>,
    /// Stories parsed from the STORY: blocks in system.arch.
    pub stories: Option<Stories>,
}

impl ArchContext {
    /// Load architecture from `.arch` source files.
    ///
    /// Reads `architecture/arch/system.arch` (system declaration, rules, stories) and
    /// `architecture/arch/containers/{id}.arch` (container + module definitions) for each
    /// declared container. Converts to the canonical `Architecture` + `ContainerDetail` types.
    pub fn load() -> Result<Self, String> {
        let root = std::env::current_dir().map_err(|e| e.to_string())?;

        let system_path = find_system_arch()?;
        let system_content = std::fs::read_to_string(&system_path)
            .map_err(|e| format!("Cannot read system.arch: {e}"))?;
        let src = arch_parser::parse_system_arch(&system_content)?;

        let stories = stories_from_source(&src);
        let mut arch = arch_source_to_architecture(&src);

        // Parse each declared container file
        let containers_dir = root.join("architecture").join("arch").join("containers");
        let mut details: HashMap<String, ContainerDetail> = HashMap::new();

        for cont_id in &src.container_ids {
            let cont_path = containers_dir.join(format!("{cont_id}.arch"));
            if !cont_path.exists() {
                continue; // validate will report this
            }
            let cont_content = std::fs::read_to_string(&cont_path)
                .map_err(|e| format!("Cannot read {cont_id}.arch: {e}"))?;
            let container = arch_parser::parse_container_arch(&cont_content, cont_id, &src.system_name)?;

            // Back-fill container metadata into the Architecture containers list
            if let Some(arch_cont) = arch.containers.iter_mut().find(|c| c.id == *cont_id) {
                arch_cont.path = container.path.clone();
                arch_cont.description = container.description.clone();
                arch_cont.depends_on = container.depends_on.clone();
                if let Some(ref proj) = container.project {
                    arch_cont.project = Some(proj.clone());
                }
            }

            details.insert(cont_id.clone(), container_to_detail(&container));
        }

        let ignore_patterns = schema::compile_ignore_patterns(&arch.system.ignore);

        Ok(ArchContext { root, arch, details, ignore_patterns, stories: Some(stories) })
    }

    /// Build the module resolution index from loaded data.
    pub fn build_index(&self) -> ModuleIndex {
        ModuleIndex::build(&self.root, &self.arch, &self.details)
    }

    /// Return stories parsed during load(). No file I/O.
    pub fn load_stories(&self) -> Result<Option<Stories>, String> {
        Ok(self.stories.clone())
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

// ────────────────────────────────────────────────────────────────────────────
// ArchSource → canonical schema type converters
// ────────────────────────────────────────────────────────────────────────────

fn arch_source_to_architecture(src: &ArchSource) -> Architecture {
    // Container shells — path/description/depends_on are back-filled from container files
    let containers = src.container_ids.iter().map(|id| Container {
        id: id.clone(),
        project: Some(src.system_name.clone()),
        path: String::new(),
        description: None,
        depends_on: Vec::new(),
        notes: None,
    }).collect();

    let rules = src.rules.iter().map(|r| Rule {
        id: r.id.clone(),
        rule_type: r.rule_type.clone(),
        from: r.from.clone(),
        to: if r.to.is_empty() {
            None
        } else {
            Some(r.to.clone())
        },
        module: r.module.clone(),
        modules: Vec::new(),
        reason: r.reason.clone(),
        constraint: r.constraint.clone(),
        pattern: None,
        allowed: r.allowed.clone(),
        allowed_max: r.allowed_max,
    }).collect();

    Architecture {
        guidance: if src.guidance.is_empty() { None } else { Some(src.guidance.clone()) },
        system: System {
            name: src.system_name.clone(),
            description: Some(src.system_description.clone()),
            ignore: src.ignore.clone(),
        },
        containers,
        rules,
    }
}

fn stories_from_source(src: &ArchSource) -> Stories {
    Stories {
        stories: src.stories.iter().map(|s| Story {
            id: s.id.clone(),
            description: s.description.clone(),
            flow: s.flow.clone(),
        }).collect(),
    }
}

fn container_to_detail(container: &ArchSourceContainer) -> ContainerDetail {
    ContainerDetail {
        modules: container.modules.iter().map(module_from_source).collect(),
        notes: None,
    }
}

fn module_from_source(m: &ArchSourceModule) -> Module {
    Module {
        id: m.id.clone(),
        file: m.file.clone().unwrap_or_default(),
        files: m.files.clone(),
        owns: m.owns.clone(),
        boundary: m.boundary.clone(),
        depends_on: m.depends_on.clone(),
        must_not_depend: Vec::new(),
        routes: None,
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


