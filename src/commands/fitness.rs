use crate::imports;
use crate::schema::{Architecture, ContainerDetail, Rule};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Serialize)]
struct RuleResult {
    rule_id: String,
    passed: bool,
    violations: Vec<String>,
}

pub fn run(json: bool) -> Result<(), String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let arch_path = crate::schema::find_arch_yaml()?;

    let content = std::fs::read_to_string(&arch_path).map_err(|e| e.to_string())?;
    let arch: Architecture =
        serde_yaml::from_str(&content).map_err(|e| format!("Invalid architecture.yaml: {e}"))?;

    if arch.rules.is_empty() {
        println!("ℹ️  No rules defined in architecture.yaml");
        return Ok(());
    }

    // Load all container details
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

    // Build actual dependency graph by scanning imports
    let actual_deps = build_actual_deps(&root, &arch, &details);

    let mut results: Vec<RuleResult> = Vec::new();

    for rule in &arch.rules {
        match rule.rule_type.as_str() {
            "no_dependency" => {
                results.push(evaluate_no_dependency(rule, &actual_deps, &arch, &details));
            }
            "no_import_from" => {
                results.push(evaluate_no_import_from(rule, &root, &arch, &details));
            }
            "boundary" => {
                // Boundary rules are prose constraints — we report them as "manual check"
                results.push(RuleResult {
                    rule_id: rule.id.clone(),
                    passed: true,
                    violations: vec![format!(
                        "(manual check) {}",
                        rule.constraint.as_deref().unwrap_or("no constraint specified")
                    )],
                });
            }
            other => {
                results.push(RuleResult {
                    rule_id: rule.id.clone(),
                    passed: true,
                    violations: vec![format!("Unknown rule type '{other}' — skipped")],
                });
            }
        }
    }

    // Report
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = results.iter().filter(|r| !r.passed).count();
    let manual = results
        .iter()
        .filter(|r| r.passed && !r.violations.is_empty())
        .count();

    if json {
        let output = serde_json::json!({
            "total": results.len(),
            "passed": passed - manual,
            "failed": failed,
            "manual": manual,
            "rules": results,
        });
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
        if failed > 0 {
            return Err(format!("{failed} rule(s) failed"));
        }
        return Ok(());
    }

    for r in &results {
        if !r.passed {
            println!("❌ {} — FAILED", r.rule_id);
            for v in &r.violations {
                println!("    {v}");
            }
        }
    }

    for r in &results {
        if r.passed && r.violations.is_empty() {
            println!("✅ {}", r.rule_id);
        }
    }

    if manual > 0 {
        println!();
        for r in &results {
            if r.passed && !r.violations.is_empty() {
                println!("📋 {} {}", r.rule_id, r.violations.first().unwrap_or(&String::new()));
            }
        }
    }

    println!(
        "\n📊 {} rule(s): {} passed, {} failed, {} manual",
        results.len(),
        passed - manual,
        failed,
        manual
    );

    if failed > 0 {
        Err(format!("{failed} rule(s) failed"))
    } else {
        Ok(())
    }
}

/// Build a map of actual dependencies: "container/module" → set of "container/module" it imports.
fn build_actual_deps(
    root: &Path,
    arch: &Architecture,
    details: &HashMap<String, ContainerDetail>,
) -> HashMap<String, HashSet<String>> {
    let mut deps: HashMap<String, HashSet<String>> = HashMap::new();

    // Build a simple resolution index: file stem and module id → container/module
    let mut id_to_full: HashMap<String, String> = HashMap::new();
    let mut stem_to_full: HashMap<String, String> = HashMap::new();
    let mut project_to_container: HashMap<String, String> = HashMap::new();

    for container in &arch.containers {
        if let Some(ref project) = container.project {
            project_to_container.insert(project.to_lowercase(), container.id.clone());
        }

        let detail = match details.get(&container.id) {
            Some(d) => d,
            None => continue,
        };

        for module in &detail.modules {
            let full_id = format!("{}/{}", container.id, module.id);
            id_to_full.insert(module.id.to_lowercase(), full_id.clone());
            let file_path = Path::new(&module.file);
            if let Some(stem) = file_path.file_stem().and_then(|s| s.to_str()) {
                stem_to_full.insert(stem.to_lowercase(), full_id.clone());
            }
        }
    }

    for container in &arch.containers {
        let detail = match details.get(&container.id) {
            Some(d) => d,
            None => continue,
        };

        for module in &detail.modules {
            let full_id = format!("{}/{}", container.id, module.id);
            let file_path = root.join(&container.path).join(&module.file);

            if !file_path.exists() {
                continue;
            }

            let file_content = match std::fs::read_to_string(&file_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let file_imports = imports::extract_imports(&file_path, &file_content);
            let module_deps = deps.entry(full_id.clone()).or_default();

            for imp in &file_imports {
                // Try to resolve to a container
                let lower = imp.raw.to_lowercase();

                // For .NET: "open AITeam.Boards" → container "boards" if project is "AITeam.Boards"
                for (project, cid) in &project_to_container {
                    if lower.starts_with(project.as_str()) || lower == *project {
                        // Find specific module within container
                        if let Some(target_detail) = details.get(cid) {
                            for target_module in &target_detail.modules {
                                let target_full =
                                    format!("{}/{}", cid, target_module.id);
                                if target_full != full_id {
                                    // Check if the import specifically references this module
                                    let after_project = if lower.len() > project.len() {
                                        &lower[project.len()..]
                                    } else {
                                        ""
                                    };
                                    let after_clean = after_project.trim_start_matches('.');
                                    if after_clean.is_empty()
                                        || after_clean
                                            .starts_with(&target_module.id.to_lowercase())
                                    {
                                        module_deps.insert(target_full);
                                    }
                                }
                            }
                        }
                        // Also record at container level
                        module_deps.insert(cid.clone());
                    }
                }

                // For Rust: "crate::schema" or "super::validate"
                let segments: Vec<&str> = if lower.contains("::") {
                    lower.split("::").collect()
                } else if lower.contains('.') {
                    lower.split('.').collect()
                } else {
                    vec![lower.as_str()]
                };

                for seg in &segments {
                    let clean = seg.replace('-', "").replace('_', "");
                    if let Some(target) = id_to_full.get(&clean) {
                        if *target != full_id {
                            module_deps.insert(target.clone());
                        }
                    }
                    if let Some(target) = stem_to_full.get(&clean) {
                        if *target != full_id {
                            module_deps.insert(target.clone());
                        }
                    }
                }
            }
        }
    }

    deps
}

fn evaluate_no_dependency(
    rule: &Rule,
    actual_deps: &HashMap<String, HashSet<String>>,
    arch: &Architecture,
    details: &HashMap<String, ContainerDetail>,
) -> RuleResult {
    let from = match &rule.from {
        Some(f) => f,
        None => {
            return RuleResult {
                rule_id: rule.id.clone(),
                passed: false,
                violations: vec!["Rule missing 'from' field".to_string()],
            }
        }
    };

    // Parse 'to' — can be a string or list of strings
    let to_targets: Vec<String> = match &rule.to {
        Some(serde_yaml::Value::String(s)) => vec![s.clone()],
        Some(serde_yaml::Value::Sequence(seq)) => seq
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        _ => {
            return RuleResult {
                rule_id: rule.id.clone(),
                passed: false,
                violations: vec!["Rule missing or invalid 'to' field".to_string()],
            }
        }
    };

    let mut violations = Vec::new();

    // Determine if 'from' is a container or container/module
    let from_modules = resolve_rule_target(from, arch, details);
    let to_module_sets: Vec<(String, HashSet<String>)> = to_targets
        .iter()
        .map(|t| (t.clone(), resolve_rule_target(t, arch, details)))
        .collect();

    for from_module in &from_modules {
        if let Some(actual) = actual_deps.get(from_module) {
            for (to_name, to_modules) in &to_module_sets {
                for to_module in to_modules {
                    if actual.contains(to_module) {
                        violations.push(format!(
                            "{from_module} → {to_module} (forbidden: {from} → {to_name})"
                        ));
                    }
                }
                // Also check container-level references
                if actual.contains(to_name) {
                    violations.push(format!(
                        "{from_module} → {to_name} (container-level reference)"
                    ));
                }
            }
        }
    }

    RuleResult {
        rule_id: rule.id.clone(),
        passed: violations.is_empty(),
        violations,
    }
}

/// Resolve a rule target (could be "container" or "container/module") to a set of full module IDs.
fn resolve_rule_target(
    target: &str,
    _arch: &Architecture,
    details: &HashMap<String, ContainerDetail>,
) -> HashSet<String> {
    let mut result = HashSet::new();

    if target.contains('/') {
        // It's a specific module: container/module
        result.insert(target.to_string());
    } else {
        // It's a container — expand to all modules in it
        if let Some(detail) = details.get(target) {
            for module in &detail.modules {
                result.insert(format!("{}/{}", target, module.id));
            }
        }
        // Also include the container itself (for container-level deps)
        result.insert(target.to_string());
    }

    result
}

/// Evaluate a no_import_from rule: no module in `from` may import anything
/// matching the glob `pattern` (checked against each import path segment).
fn evaluate_no_import_from(
    rule: &Rule,
    root: &Path,
    arch: &Architecture,
    details: &HashMap<String, ContainerDetail>,
) -> RuleResult {
    let from = match &rule.from {
        Some(f) => f,
        None => {
            return RuleResult {
                rule_id: rule.id.clone(),
                passed: false,
                violations: vec!["Rule missing 'from' field".to_string()],
            }
        }
    };

    let pattern_str = match &rule.pattern {
        Some(p) => p,
        None => {
            return RuleResult {
                rule_id: rule.id.clone(),
                passed: false,
                violations: vec!["Rule missing 'pattern' field".to_string()],
            }
        }
    };

    let pattern = match glob::Pattern::new(pattern_str) {
        Ok(p) => p,
        Err(e) => {
            return RuleResult {
                rule_id: rule.id.clone(),
                passed: false,
                violations: vec![format!("Invalid glob pattern '{}': {}", pattern_str, e)],
            }
        }
    };

    let from_modules = resolve_rule_target(from, arch, details);
    let mut violations = Vec::new();

    for container in &arch.containers {
        let detail = match details.get(&container.id) {
            Some(d) => d,
            None => continue,
        };

        for module in &detail.modules {
            let full_id = format!("{}/{}", container.id, module.id);
            if !from_modules.contains(&full_id) && !from_modules.contains(&container.id) {
                continue;
            }

            let file_path = root.join(&container.path).join(&module.file);
            if !file_path.exists() {
                continue;
            }

            let file_content = match std::fs::read_to_string(&file_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let file_imports = imports::extract_imports(&file_path, &file_content);

            for imp in &file_imports {
                // Normalize import: replace . and :: with / for segment matching
                let normalized = imp.raw.replace('.', "/").replace("::", "/");
                let segments: Vec<&str> = normalized.split('/').collect();

                // Match pattern against each segment or the full normalized path
                let matches = segments.iter().any(|s| pattern.matches(s))
                    || pattern.matches(&normalized);

                if matches {
                    violations.push(format!(
                        "{} ({}:{}) imports `{}`",
                        full_id, module.file, imp.line_number, imp.raw
                    ));
                }
            }
        }
    }

    RuleResult {
        rule_id: rule.id.clone(),
        passed: violations.is_empty(),
        violations,
    }
}
