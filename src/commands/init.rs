use crate::scanner;
use std::path::Path;

pub fn run(deep: bool) -> Result<(), String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;

    if root.join("architecture").join("architecture.yaml").exists() || root.join("architecture.yaml").exists() {
        return Err("architecture.yaml already exists. Delete it first to reinitialize.".into());
    }

    let project_type = scanner::detect_project_type(&root);
    let type_name = match project_type {
        scanner::ProjectType::Rust => "Rust",
        scanner::ProjectType::DotNet => ".NET",
        scanner::ProjectType::JavaScript => "JavaScript/TypeScript",
        scanner::ProjectType::Python => "Python",
        scanner::ProjectType::Go => "Go",
        scanner::ProjectType::Generic => "Generic",
    };

    println!("Detected project type: {type_name}");
    if deep {
        println!("Deep scan: inferring modules from project files...");
    } else {
        println!("Scanning project structure...");
    }

    let project_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("MyProject");

    // Create architecture/ directory
    let arch_dir = root.join("architecture");
    std::fs::create_dir_all(&arch_dir).map_err(|e| e.to_string())?;

    // Scan for containers first (needed for root YAML)
    let containers = scan_containers(&root, &project_type);

    // Generate root architecture.yaml inside architecture/
    let yaml = generate_root_yaml(project_name, &containers);
    std::fs::write(arch_dir.join("architecture.yaml"), yaml).map_err(|e| e.to_string())?;

    // Generate stories.yaml stub
    let stories = generate_stories_stub();
    std::fs::write(arch_dir.join("stories.yaml"), stories).map_err(|e| e.to_string())?;

    // Generate per-container detail files
    for (id, path) in &containers {
        let container_yaml = if deep {
            generate_deep_container_yaml(&root, id, path)
        } else {
            generate_container_yaml(id, path)
        };
        std::fs::write(arch_dir.join(format!("{id}.yaml")), container_yaml)
            .map_err(|e| e.to_string())?;
    }

    println!("\nCreated:");
    println!("  architecture/architecture.yaml");
    println!("  architecture/stories.yaml");
    for (id, _) in &containers {
        println!("  architecture/{id}.yaml");
    }
    println!("\nNext: review the generated files and fill in ownership details.");

    Ok(())
}

fn generate_root_yaml(name: &str, containers: &[(String, String)]) -> String {
    let mut yaml = format!(
        r#"guidance: |
  Before making code changes, read the relevant container YAML.
  After changes, update ownership and dependencies if they changed.
  Check stories.yaml if your change crosses multiple modules.

system:
  name: {name}
  description: TODO — describe your project

containers:
"#
    );

    if containers.is_empty() {
        yaml.push_str("  []\n");
    } else {
        for (id, path) in containers {
            yaml.push_str(&format!(
                "  - id: {id}\n    path: {path}\n    description: TODO\n    depends_on: []\n\n"
            ));
        }
    }

    yaml.push_str(
        r#"rules: []
  # Example:
  # - id: core-independence
  #   type: no_dependency
  #   from: core
  #   to: [ui, api]
  #   reason: "Core must not depend on presentation layers"
"#,
    );

    yaml
}

fn generate_stories_stub() -> String {
    r#"# Cross-cutting flows connecting modules across containers.
# Add stories to document how features work end-to-end.

stories: []
  # Example:
  # - id: user-login
  #   description: User submits credentials, validated, token issued
  #   flow:
  #     - frontend/login-page
  #     - backend/auth
  #     - backend/database
"#
    .to_string()
}

fn scan_containers(root: &Path, _project_type: &scanner::ProjectType) -> Vec<(String, String)> {
    // Look for common source directories
    let candidates = ["src", "lib", "app", "packages", "crates", "services"];
    let mut containers = Vec::new();

    for candidate in &candidates {
        let dir = root.join(candidate);
        if dir.is_dir() {
            // Check if this dir has subdirectories (potential containers)
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    if entry.path().is_dir() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let path = format!("{candidate}/{name}");
                        containers.push((name.to_lowercase().replace(' ', "-"), path));
                    }
                }
            }

            // If no subdirs, the candidate itself is a container
            if containers.is_empty() {
                containers.push((candidate.to_string(), candidate.to_string()));
            }
            break;
        }
    }

    if containers.is_empty() {
        containers.push(("main".to_string(), ".".to_string()));
    }

    containers
}

fn generate_container_yaml(id: &str, path: &str) -> String {
    format!(
        r#"# {id} — TODO: describe this container
# Source: {path}

modules: []
  # Example:
  # - id: auth
  #   file: auth.rs
  #   owns: [authentication, token-validation]
  #   boundary: "Auth logic only — no direct DB queries"
  #   depends_on: [backend/database]
"#
    )
}

/// Deep scan: find .csproj/.fsproj files and __init__.py packages,
/// infer modules with depends_on from ProjectReference tags.
fn generate_deep_container_yaml(root: &Path, id: &str, rel_path: &str) -> String {
    let container_dir = root.join(rel_path);
    let mut modules: Vec<InferredModule> = Vec::new();

    if container_dir.is_dir() {
        // Scan for .NET project files
        for entry in walkdir::WalkDir::new(&container_dir)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_str().unwrap_or("");
                !e.file_type().is_dir() || !crate::schema::SKIP_DIRS.contains(&name)
            })
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            match ext {
                "csproj" | "fsproj" | "vbproj" => {
                    if let Some(m) = infer_dotnet_module(&container_dir, path) {
                        modules.push(m);
                    }
                }
                _ => {}
            }
        }

        // Scan for Python packages (__init__.py)
        if modules.is_empty() {
            for entry in walkdir::WalkDir::new(&container_dir)
                .max_depth(3)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
            {
                if entry.file_name() == "__init__.py" {
                    if let Some(m) = infer_python_module(&container_dir, entry.path()) {
                        modules.push(m);
                    }
                }
            }
        }
    }

    if modules.is_empty() {
        return generate_container_yaml(id, rel_path);
    }

    let mut yaml = format!("# {id} — TODO: describe this container\n# Source: {rel_path}\n\nmodules:\n");

    for m in &modules {
        yaml.push_str(&format!("  - id: {}\n", m.id));
        yaml.push_str(&format!("    file: {}\n", m.file));
        yaml.push_str(&format!("    owns: [{}]\n", m.id)); // placeholder
        if !m.depends_on.is_empty() {
            yaml.push_str(&format!("    depends_on: [{}]\n", m.depends_on.join(", ")));
        }
        yaml.push('\n');
    }

    yaml
}

struct InferredModule {
    id: String,
    file: String,
    depends_on: Vec<String>,
}

/// Infer a module from a .csproj/.fsproj file.
/// Reads <ProjectReference> tags to populate depends_on.
fn infer_dotnet_module(container_dir: &Path, proj_path: &Path) -> Option<InferredModule> {
    let rel = proj_path.strip_prefix(container_dir).ok()?;
    let file = rel.to_string_lossy().replace('\\', "/");
    let stem = proj_path.file_stem()?.to_str()?;

    // Derive module ID from project name (kebab-case)
    let id = stem_to_kebab(stem);

    // Parse ProjectReference from the .csproj XML
    let content = std::fs::read_to_string(proj_path).ok()?;
    let deps = parse_project_references(&content);

    Some(InferredModule {
        id,
        file,
        depends_on: deps,
    })
}

/// Infer a module from a Python __init__.py.
fn infer_python_module(container_dir: &Path, init_path: &Path) -> Option<InferredModule> {
    let parent = init_path.parent()?;
    let rel = init_path.strip_prefix(container_dir).ok()?;
    let file = rel.to_string_lossy().replace('\\', "/");
    let name = parent.file_name()?.to_str()?;
    let id = stem_to_kebab(name);

    Some(InferredModule {
        id,
        file,
        depends_on: vec![],
    })
}

/// Convert a PascalCase or dot-separated project name to kebab-case.
/// e.g., "DataProcessing.Domain" → "domain", "MyApp" → "my-app"
fn stem_to_kebab(s: &str) -> String {
    // Use the last dot-segment if present
    let base = s.rsplit('.').next().unwrap_or(s);

    // Insert hyphens before uppercase runs
    let mut result = String::new();
    for (i, ch) in base.chars().enumerate() {
        if i > 0 && ch.is_uppercase() && !base.chars().nth(i - 1).unwrap_or('A').is_uppercase() {
            result.push('-');
        }
        result.push(ch.to_lowercase().next().unwrap_or(ch));
    }
    result
}

/// Parse <ProjectReference Include="..."> from a .csproj file.
/// Returns kebab-case module IDs derived from referenced project names.
fn parse_project_references(content: &str) -> Vec<String> {
    let mut refs = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.contains("ProjectReference") && trimmed.contains("Include=") {
            // Extract the Include="..." value
            if let Some(start) = trimmed.find("Include=\"") {
                let rest = &trimmed[start + 9..];
                if let Some(end) = rest.find('"') {
                    let path = &rest[..end];
                    // Get the project name from the path
                    let proj_name = Path::new(path)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("");
                    if !proj_name.is_empty() {
                        refs.push(stem_to_kebab(proj_name));
                    }
                }
            }
        }
    }
    refs
}
