use crate::scanner;
use std::path::Path;

pub fn run() -> Result<(), String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;

    if root.join("architecture.yaml").exists() {
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
    println!("Scanning project structure...");

    let project_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("MyProject");

    // Generate root architecture.yaml
    let yaml = generate_root_yaml(project_name);
    std::fs::write(root.join("architecture.yaml"), yaml).map_err(|e| e.to_string())?;

    // Create architecture/ directory
    let arch_dir = root.join("architecture");
    std::fs::create_dir_all(&arch_dir).map_err(|e| e.to_string())?;

    // Generate stories.yaml stub
    let stories = generate_stories_stub();
    std::fs::write(arch_dir.join("stories.yaml"), stories).map_err(|e| e.to_string())?;

    // Scan for containers and generate per-container files
    let containers = scan_containers(&root, &project_type);
    for (id, path) in &containers {
        let container_yaml = generate_container_yaml(id, path);
        std::fs::write(arch_dir.join(format!("{id}.yaml")), container_yaml)
            .map_err(|e| e.to_string())?;
    }

    println!("\nCreated:");
    println!("  architecture.yaml");
    println!("  architecture/stories.yaml");
    for (id, _) in &containers {
        println!("  architecture/{id}.yaml");
    }
    println!("\nNext: review the generated files and fill in ownership details.");

    Ok(())
}

fn generate_root_yaml(name: &str) -> String {
    format!(
        r#"guidance: |
  Before making code changes, read the relevant container YAML.
  After changes, update ownership and dependencies if they changed.
  Check stories.yaml if your change crosses multiple modules.

system:
  name: {name}
  description: TODO — describe your project

containers: []
  # Populated by arch init — edit to add depends_on and descriptions

rules: []
  # Example:
  # - id: core-independence
  #   type: no_dependency
  #   from: core
  #   to: [ui, api]
  #   reason: "Core must not depend on presentation layers"
"#
    )
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
