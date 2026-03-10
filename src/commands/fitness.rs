use crate::context::{ArchContext, print_json};
use crate::depgraph;
use crate::imports;
use crate::schema::{Architecture, ContainerDetail, Rule};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Serialize)]
struct RuleResult {
    rule_id: String,
    passed: bool,
    violations: Vec<String>,
}

pub fn run(json: bool) -> Result<(), String> {
    let ctx = ArchContext::load()?;

    if ctx.arch.rules.is_empty() {
        println!("ℹ️  No rules defined in architecture.yaml");
        return Ok(());
    }

    // Use the shared ModuleIndex instead of a parallel resolver
    let index = ctx.build_index();
    let actual_deps = depgraph::build_dep_graph(&ctx, &index, false);

    let mut results: Vec<RuleResult> = Vec::new();

    for rule in &ctx.arch.rules {
        match rule.rule_type.as_str() {
            "no_dependency" => {
                results.push(evaluate_no_dependency(rule, &actual_deps, &ctx.arch, &ctx.details));
            }
            "no_import_from" => {
                results.push(evaluate_no_import_from(rule, &ctx));
            }
            // ASSUMPTION: boundary rules are not automatically enforceable — they are advisory/documentary only.
            // IF THIS CHANGES: implement structural boundary checking by comparing module file ownership
            // against declared container paths and flagging cross-boundary file references.
            "boundary" => {
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

    report_fitness(json, &results)
}

fn report_fitness(json: bool, results: &[RuleResult]) -> Result<(), String> {
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
        print_json(&output)?;
        if failed > 0 {
            return Err(format!("{failed} rule(s) failed"));
        }
        return Ok(());
    }

    for r in results {
        if !r.passed {
            println!("❌ {} — FAILED", r.rule_id);
            for v in &r.violations {
                println!("    {v}");
            }
        }
    }

    for r in results {
        if r.passed && r.violations.is_empty() {
            println!("✅ {}", r.rule_id);
        }
    }

    if manual > 0 {
        println!();
        for r in results {
            if r.passed && !r.violations.is_empty() {
                println!("📋 {} {}", r.rule_id, r.violations.first().map(|s| s.as_str()).unwrap_or(""));
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

fn evaluate_no_dependency(
    rule: &Rule,
    actual_deps: &HashMap<String, HashSet<String>>,
    _arch: &Architecture,
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

    let from_modules = resolve_rule_target(from, details);
    let to_module_sets: Vec<(String, HashSet<String>)> = to_targets
        .iter()
        .map(|t| (t.clone(), resolve_rule_target(t, details)))
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
    details: &HashMap<String, ContainerDetail>,
) -> HashSet<String> {
    let mut result = HashSet::new();

    if target.contains('/') {
        result.insert(target.to_string());
    } else {
        if let Some(detail) = details.get(target) {
            for module in &detail.modules {
                result.insert(format!("{}/{}", target, module.id));
            }
        }
        result.insert(target.to_string());
    }

    result
}

fn evaluate_no_import_from(
    rule: &Rule,
    ctx: &ArchContext,
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

    let from_modules = resolve_rule_target(from, &ctx.details);
    let mut violations = Vec::new();

    for from_module in &from_modules {
        let parts: Vec<&str> = from_module.split('/').collect();
        if parts.len() != 2 {
            continue;
        }
        let (container_id, module_id) = (parts[0], parts[1]);

        let container = match ctx.arch.containers.iter().find(|c| c.id == container_id) {
            Some(c) => c,
            None => continue,
        };

        let detail = match ctx.details.get(container_id) {
            Some(d) => d,
            None => continue,
        };

        let module = match detail.modules.iter().find(|m| m.id == module_id) {
            Some(m) => m,
            None => continue,
        };

        for file in module.all_files() {
            let file_path = ctx.root.join(&container.path).join(file);
            if !file_path.exists() {
                continue;
            }

            let file_content = match std::fs::read_to_string(&file_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let file_imports = imports::extract_imports(&file_path, &file_content);
            for imp in &file_imports {
                let lower = imp.raw.to_lowercase();
                let segments: Vec<&str> = if lower.contains("::") {
                    lower.split("::").collect()
                } else if lower.contains('.') {
                    lower.split('.').collect()
                } else {
                    vec![lower.as_str()]
                };

                for seg in &segments {
                    if pattern.matches(seg) {
                        violations.push(format!(
                            "{from_module} imports '{}' (matches pattern '{pattern_str}')",
                            imp.raw
                        ));
                        break;
                    }
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
