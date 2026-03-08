/// Project type detection and scanning for `arch init`

pub enum ProjectType {
    Rust,
    DotNet,
    JavaScript,
    Python,
    Go,
    Generic,
}

pub fn detect_project_type(root: &std::path::Path) -> ProjectType {
    if root.join("Cargo.toml").exists() {
        ProjectType::Rust
    } else if has_extension(root, "sln") || has_extension(root, "csproj") || has_extension(root, "fsproj") {
        ProjectType::DotNet
    } else if root.join("package.json").exists() {
        ProjectType::JavaScript
    } else if root.join("pyproject.toml").exists() || root.join("setup.py").exists() {
        ProjectType::Python
    } else if root.join("go.mod").exists() {
        ProjectType::Go
    } else {
        ProjectType::Generic
    }
}

fn has_extension(root: &std::path::Path, ext: &str) -> bool {
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            if let Some(e) = entry.path().extension() {
                if e == ext {
                    return true;
                }
            }
        }
    }
    false
}
