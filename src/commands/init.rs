use crate::scanner;
use std::path::Path;

pub fn run(deep: bool) -> Result<(), String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;

    let system_arch_path = root.join("architecture").join("arch").join("system.arch");
    if system_arch_path.exists() {
        return Err("architecture/arch/system.arch already exists. Delete it first to reinitialize.".into());
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

    // Create architecture/arch/containers/ directories
    let arch_dir = root.join("architecture").join("arch");
    let containers_dir = arch_dir.join("containers");
    std::fs::create_dir_all(&containers_dir).map_err(|e| e.to_string())?;

    // Scan for containers first (needed for system.arch CONT: list)
    let containers = scan_containers(&root, &project_type);

    // Write system.arch
    let system_arch = generate_system_arch(project_name, &containers);
    std::fs::write(&system_arch_path, system_arch).map_err(|e| e.to_string())?;

    // Write per-container .arch files
    for (id, path) in &containers {
        let container_arch = if deep {
            generate_deep_container_arch(&root, id, path)
        } else {
            generate_container_arch(id, path)
        };
        std::fs::write(containers_dir.join(format!("{id}.arch")), container_arch)
            .map_err(|e| e.to_string())?;
    }

    println!("\nCreated:");
    println!("  architecture/arch/system.arch");
    for (id, _) in &containers {
        println!("  architecture/arch/containers/{id}.arch");
    }
    println!("\nNext: review the generated files and fill in ownership details.");

    Ok(())
}

fn generate_system_arch(name: &str, containers: &[(String, String)]) -> String {
    let mut out = format!(
        "ARCH: 0.4\nSYS: {name}\nDESC: TODO — describe your project\n\nGUIDE:\n  Before making code changes, read the relevant container .arch file.\n  After changes, update ownership and dependencies if they changed.\n  Check STORY: blocks if your change crosses multiple modules.\nENDGUIDE\n\n"
    );

    for (id, _) in containers {
        out.push_str(&format!("CONT: {id}\n"));
    }

    out.push_str(
        "\n# Add fitness rules here. Example:\n# RULE: core-independence\n#   TYPE: no_dependency\n#   FROM: core/core\n#   TO: ui/ui\n#   WHY: Core must not depend on presentation layers\n\n# Add stories here. Example:\n# STORY: user-login\n#   DESC: User submits credentials, validated, token issued\n#   FLOW: frontend/login; backend/auth; backend/database\n",
    );

    out
}

fn generate_container_arch(id: &str, path: &str) -> String {
    format!(
        "ARCH: 0.4\nCONT: {id}\nPATH: {path}\nDEP:\nDESC: TODO — describe this container\n\n# MOD: module-name\n#   FILE: some/file.rs\n#   OWN: concept1; concept2\n#   BND: optional boundary description\n#   DEP: other-container/module\n"
    )
}

fn generate_deep_container_arch(root: &Path, id: &str, rel_path: &str) -> String {
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

        // Scan for Python packages (__init__.py) — always scan, not just when .NET is empty
        for entry in walkdir::WalkDir::new(&container_dir)
            .max_depth(4)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_str().unwrap_or("");
                !e.file_type().is_dir() || !crate::schema::SKIP_DIRS.contains(&name)
            })
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            if entry.file_name() == "__init__.py" {
                if let Some(m) = infer_python_module(&container_dir, entry.path()) {
                    if !modules.iter().any(|existing| existing.id == m.id) {
                        modules.push(m);
                    }
                }
            }
        }

        // Scan for Rust modules (mod.rs or lib.rs) — only if no modules found yet
        if modules.is_empty() {
            for entry in walkdir::WalkDir::new(&container_dir)
                .max_depth(3)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
            {
                let name = entry.file_name().to_str().unwrap_or("");
                if name == "mod.rs" || name == "lib.rs" {
                    if let Some(m) = infer_rust_module(&container_dir, entry.path()) {
                        modules.push(m);
                    }
                }
            }
        }
    }

    if modules.is_empty() {
        return generate_container_arch(id, rel_path);
    }

    // Deduplicate: use parent-prefix compound naming (e.g., durable-tasks--in-memory)
    let mut id_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for m in &modules {
        *id_counts.entry(m.id.clone()).or_default() += 1;
    }
    let duplicated_ids: std::collections::HashSet<String> = id_counts.into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(id, _)| id)
        .collect();
    if !duplicated_ids.is_empty() {
        for m in &mut modules {
            if duplicated_ids.contains(&m.id) {
                let stem = std::path::Path::new(&m.file)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                let segments: Vec<&str> = stem.split('.').collect();
                if segments.len() >= 2 {
                    let parent = pascal_to_kebab(segments[segments.len() - 2]);
                    let child = pascal_to_kebab(segments[segments.len() - 1]);
                    m.id = format!("{}--{}", parent, child);
                }
            }
        }

        let mut final_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for m in &modules {
            *final_counts.entry(m.id.clone()).or_default() += 1;
        }
        let still_duped: std::collections::HashSet<String> = final_counts.into_iter()
            .filter(|(_, count)| *count > 1)
            .map(|(id, _)| id)
            .collect();
        if !still_duped.is_empty() {
            let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for m in &mut modules {
                if still_duped.contains(&m.id) {
                    let n = seen.entry(m.id.clone()).or_default();
                    *n += 1;
                    if *n > 1 {
                        m.id = format!("{}-{}", m.id, n);
                    }
                }
            }
        }
    }

    // Emit .arch format
    let mut out = format!("ARCH: 0.4\nCONT: {id}\nPATH: {rel_path}\nDEP:\nDESC: TODO — describe this container\n\n");

    for m in &modules {
        out.push_str(&format!("MOD: {}\n", m.id));
        out.push_str(&format!("  FILE: {}\n", m.file));
        if !m.owns.is_empty() {
            out.push_str(&format!("  OWN: {}\n", m.owns.join("; ")));
        }
        if let Some(ref boundary) = m.boundary {
            out.push_str(&format!("  BND: {boundary}\n"));
        }
        if !m.depends_on.is_empty() {
            out.push_str(&format!("  DEP: {}\n", m.depends_on.join("; ")));
        }
        out.push('\n');
    }

    out
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

struct InferredModule {
    id: String,
    file: String,
    owns: Vec<String>,
    boundary: Option<String>,
    depends_on: Vec<String>,
}

/// Classify a .NET project by naming convention.
fn classify_dotnet_project(stem: &str) -> (&'static str, Option<&'static str>) {
    let lower = stem.to_lowercase();
    if lower.ends_with(".tests") || lower.ends_with(".test") || lower.ends_with(".unittests") {
        ("test", Some("Test code only — no production logic"))
    } else if lower.ends_with(".contracts") || lower.ends_with(".abstractions") || lower.ends_with(".interfaces") {
        ("contracts", Some("Pure interfaces and DTOs — no implementation"))
    } else if lower.ends_with(".application") || lower.ends_with(".service") || lower.ends_with(".host") {
        ("service-host", Some("Application entry point and hosting"))
    } else if lower.ends_with(".domain") || lower.ends_with(".core") {
        ("domain", Some("Core business logic — no infrastructure dependencies"))
    } else if lower.ends_with(".infrastructure") || lower.ends_with(".data") {
        ("infrastructure", Some("Infrastructure and data access"))
    } else if lower.contains(".client") || lower.contains(".generated") {
        ("generated-client", Some("Generated code — do not edit manually"))
    } else {
        ("library", None)
    }
}

/// Derive owns concepts from project name segments.
fn infer_owns(stem: &str, classification: &str) -> Vec<String> {
    let mut owns = Vec::new();

    // Split by dots and take meaningful segments (skip common prefixes)
    let segments: Vec<&str> = stem.split('.').collect();
    for seg in &segments {
        let kebab = stem_to_kebab(seg);
        if !kebab.is_empty() && kebab.len() > 2 {
            owns.push(kebab);
        }
    }

    // Add classification as an own if not already present
    if !owns.iter().any(|o| o == classification) && classification != "library" {
        owns.push(classification.to_string());
    }

    if owns.is_empty() {
        owns.push(stem_to_kebab(stem));
    }

    owns.dedup();
    owns
}

/// Infer a module from a .csproj/.fsproj file.
/// Reads <ProjectReference> tags to populate depends_on.
fn infer_dotnet_module(container_dir: &Path, proj_path: &Path) -> Option<InferredModule> {
    let rel = proj_path.strip_prefix(container_dir).ok()?;
    let file = rel.to_string_lossy().replace('\\', "/");

    // Skip template/placeholder projects (paths with {variable} or __variable__ tokens)
    if (file.contains('{') && file.contains('}')) || file.contains("__") {
        return None;
    }

    let stem = proj_path.file_stem()?.to_str()?;
    let (classification, boundary) = classify_dotnet_project(stem);

    // Use full stem for classified projects to disambiguate (e.g., "shared-tests" not "tests")
    let id = if classification != "library" {
        stem_to_kebab_full(stem)
    } else {
        stem_to_kebab(stem)
    };

    let owns = infer_owns(stem, classification);
    let content = std::fs::read_to_string(proj_path).ok()?;
    let deps = parse_project_references(&content);

    Some(InferredModule {
        id,
        file,
        owns,
        boundary: boundary.map(|s| s.to_string()),
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

    // Try to read pyproject.toml in the container dir for dependency info
    let deps = find_python_deps(container_dir, name);

    Some(InferredModule {
        id: id.clone(),
        file,
        owns: vec![id],
        boundary: None,
        depends_on: deps,
    })
}

/// Infer a module from a Rust mod.rs or lib.rs.
fn infer_rust_module(container_dir: &Path, mod_path: &Path) -> Option<InferredModule> {
    let parent = mod_path.parent()?;
    let rel = mod_path.strip_prefix(container_dir).ok()?;
    let file = rel.to_string_lossy().replace('\\', "/");
    let name = if mod_path.file_name()?.to_str()? == "lib.rs" {
        container_dir.file_name()?.to_str()?
    } else {
        parent.file_name()?.to_str()?
    };
    let id = stem_to_kebab(name);

    Some(InferredModule {
        id: id.clone(),
        file,
        owns: vec![id],
        boundary: None,
        depends_on: vec![],
    })
}

/// Look for pyproject.toml and extract internal dependencies for a given package.
fn find_python_deps(container_dir: &Path, _package_name: &str) -> Vec<String> {
    // Walk up to find pyproject.toml
    let pyproject = container_dir.join("pyproject.toml");
    if !pyproject.exists() {
        return vec![];
    }

    let content = match std::fs::read_to_string(&pyproject) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    // Simple parsing: look for dependencies = [...] lines
    let mut deps = Vec::new();
    let mut in_deps = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("dependencies") && trimmed.contains('[') {
            in_deps = true;
            // Check for inline array
            if let Some(start) = trimmed.find('[') {
                let rest = &trimmed[start + 1..];
                if let Some(end) = rest.find(']') {
                    for dep in rest[..end].split(',') {
                        let dep = dep.trim().trim_matches('"').trim_matches('\'');
                        let name = dep.split(['>', '<', '=', '!', '~', ';', ' ']).next().unwrap_or("");
                        if !name.is_empty() {
                            deps.push(stem_to_kebab(name));
                        }
                    }
                    in_deps = false;
                }
            }
            continue;
        }
        if in_deps {
            if trimmed == "]" {
                in_deps = false;
                continue;
            }
            let dep = trimmed.trim_matches('"').trim_matches('\'').trim_matches(',');
            let name = dep.split(['>', '<', '=', '!', '~', ';', ' ']).next().unwrap_or("");
            if !name.is_empty() {
                deps.push(stem_to_kebab(name));
            }
        }
    }

    deps
}

/// Convert a PascalCase or dot-separated project name to kebab-case module ID.
/// For classified projects (tests, contracts, etc.), uses more stem segments to disambiguate.
/// e.g., "Shared.Tests" → "shared-tests", "Foundation.Tests" → "foundation-tests"
/// e.g., "DataProcessing.Domain" → "domain", "MyApp" → "my-app"
fn stem_to_kebab(s: &str) -> String {
    // Use the last dot-segment if present
    let base = s.rsplit('.').next().unwrap_or(s);
    pascal_to_kebab(base)
}

/// Like stem_to_kebab but uses last two dot-segments for disambiguation.
/// e.g., "Shared.Tests" → "shared-tests", "DurableTasks.Tests" → "durable-tasks-tests"
fn stem_to_kebab_full(s: &str) -> String {
    let segments: Vec<&str> = s.split('.').collect();
    if segments.len() >= 2 {
        let base = format!("{}-{}", segments[segments.len() - 2], segments[segments.len() - 1]);
        pascal_to_kebab(&base)
    } else {
        pascal_to_kebab(s)
    }
}

/// Convert PascalCase to kebab-case: "DataProcessing" → "data-processing"
fn pascal_to_kebab(s: &str) -> String {
    let mut result = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && ch.is_uppercase() && !s.chars().nth(i - 1).unwrap_or('A').is_uppercase() {
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
