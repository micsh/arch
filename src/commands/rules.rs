use crate::context::{ArchContext, print_json};
use crate::schema::Rule;

/// Extract the `to` field as a list of strings.
fn to_list(rule: &Rule) -> Vec<String> {
    rule.to.clone().unwrap_or_default()
}

pub fn run(module: Option<&str>, json: bool) -> Result<(), String> {
    let ctx = ArchContext::load()?;
    let rules = &ctx.arch.rules;

    if rules.is_empty() {
        if json {
            print_json(&serde_json::json!({ "total": 0, "rules": [] }))?;
        } else {
            println!("ℹ️  No rules defined in architecture.yaml");
        }
        return Ok(());
    }

    if let Some(module_name) = module {
        run_filtered(module_name, rules, json)
    } else {
        run_all(rules, json)
    }
}

/// Show all rules that reference a specific module.
fn run_filtered(module_name: &str, rules: &[Rule], json: bool) -> Result<(), String> {
    // no_dependency: this module is the `from` (what it cannot depend on)
    let from_rules: Vec<&Rule> = rules
        .iter()
        .filter(|r| r.rule_type == "no_dependency" && r.from.as_deref() == Some(module_name))
        .collect();

    // no_dependency: this module appears in `to` (what cannot depend on it)
    let to_rules: Vec<&Rule> = rules
        .iter()
        .filter(|r| r.rule_type == "no_dependency" && to_list(r).iter().any(|t| t == module_name))
        .collect();

    // boundary: this module is named in `modules:` list or singular `module:`
    let boundary_rules: Vec<&Rule> = rules
        .iter()
        .filter(|r| {
            r.rule_type == "boundary"
                && (r.modules.iter().any(|m| m == module_name)
                    || r.module.as_deref() == Some(module_name))
        })
        .collect();

    // no_import_from: this module is the `from` (forbidden import patterns)
    let import_from_rules: Vec<&Rule> = rules
        .iter()
        .filter(|r| r.rule_type == "no_import_from" && r.from.as_deref() == Some(module_name))
        .collect();

    // restrict_callers_to: this module is the protected target
    let restrict_callers_rules: Vec<&Rule> = rules
        .iter()
        .filter(|r| {
            r.rule_type == "restrict_callers_to" && r.module.as_deref() == Some(module_name)
        })
        .collect();

    if json {
        let cannot_depend_on: Vec<serde_json::Value> = from_rules
            .iter()
            .map(|r| {
                serde_json::json!({
                    "rule_id": r.id,
                    "forbidden_targets": to_list(r),
                    "reason": r.reason,
                })
            })
            .collect();
        let forbidden_dependents: Vec<serde_json::Value> = to_rules
            .iter()
            .map(|r| {
                serde_json::json!({
                    "rule_id": r.id,
                    "from_module": r.from,
                    "reason": r.reason,
                })
            })
            .collect();
        let boundary: Vec<serde_json::Value> = boundary_rules
            .iter()
            .map(|r| {
                serde_json::json!({
                    "rule_id": r.id,
                    "constraint": r.constraint,
                    "reason": r.reason,
                })
            })
            .collect();
        let no_import_from: Vec<serde_json::Value> = import_from_rules
            .iter()
            .map(|r| {
                serde_json::json!({
                    "rule_id": r.id,
                    "pattern": r.pattern,
                    "reason": r.reason,
                })
            })
            .collect();
        let restrict_callers: Vec<serde_json::Value> = restrict_callers_rules
            .iter()
            .map(|r| {
                serde_json::json!({
                    "rule_id": r.id,
                    "allowed_callers": r.allowed,
                    "reason": r.reason,
                })
            })
            .collect();
        print_json(&serde_json::json!({
            "module": module_name,
            "cannot_depend_on": cannot_depend_on,
            "forbidden_dependents": forbidden_dependents,
            "boundary": boundary,
            "no_import_from": no_import_from,
            "restrict_callers_to": restrict_callers,
        }))?;
        return Ok(());
    }

    if from_rules.is_empty() && to_rules.is_empty()
        && boundary_rules.is_empty() && import_from_rules.is_empty()
        && restrict_callers_rules.is_empty()
    {
        println!("No rules reference module '{module_name}'");
        return Ok(());
    }

    println!("Rules applying to: {module_name}");

    for r in &from_rules {
        let targets = to_list(r).join(", ");
        println!("  ❌ cannot depend on: {targets}");
        if let Some(reason) = &r.reason {
            println!("     {} — \"{reason}\"", r.id);
        } else {
            println!("     {}", r.id);
        }
    }

    if !to_rules.is_empty() {
        if !from_rules.is_empty() {
            println!();
        }
        println!("  ⚠️  other modules forbidden to depend on {module_name}:");
        for r in &to_rules {
            let from = r.from.as_deref().unwrap_or("?");
            if let Some(reason) = &r.reason {
                println!("     {from}  ({} — \"{reason}\")", r.id);
            } else {
                println!("     {from}  ({})", r.id);
            }
        }
    }

    if !boundary_rules.is_empty() {
        if !from_rules.is_empty() || !to_rules.is_empty() {
            println!();
        }
        println!("  📋 boundary constraints:");
        for r in &boundary_rules {
            if let Some(constraint) = &r.constraint {
                println!("     {} — \"{constraint}\"", r.id);
            } else {
                println!("     {}", r.id);
            }
            if let Some(reason) = &r.reason {
                println!("     reason: {reason}");
            }
        }
    }

    if !import_from_rules.is_empty() {
        if !from_rules.is_empty() || !to_rules.is_empty() || !boundary_rules.is_empty() {
            println!();
        }
        println!("  🚫 forbidden import patterns:");
        for r in &import_from_rules {
            let pattern = r.pattern.as_deref().unwrap_or("?");
            println!("     {} — cannot import from '{pattern}'", r.id);
            if let Some(reason) = &r.reason {
                println!("     reason: {reason}");
            }
        }
    }

    if !restrict_callers_rules.is_empty() {
        if !from_rules.is_empty() || !to_rules.is_empty()
            || !boundary_rules.is_empty() || !import_from_rules.is_empty()
        {
            println!();
        }
        println!("  🔒 restrict callers to allowlist:");
        for r in &restrict_callers_rules {
            if r.allowed.is_empty() {
                println!("     {} — ⚠️  allowed list is empty", r.id);
            } else {
                println!("     {} — only: {}", r.id, r.allowed.join(", "));
            }
            if let Some(reason) = &r.reason {
                println!("     reason: {reason}");
            }
        }
    }

    Ok(())
}

/// List all rules.
fn run_all(rules: &[Rule], json: bool) -> Result<(), String> {
    if json {
        let rule_list: Vec<serde_json::Value> = rules
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "type": r.rule_type,
                    "from": r.from,
                    "to": to_list(r),
                    "modules": r.modules,
                    "reason": r.reason,
                    "constraint": r.constraint,
                })
            })
            .collect();
        print_json(&serde_json::json!({
            "total": rules.len(),
            "rules": rule_list,
        }))?;
        return Ok(());
    }

    println!("📋 {} rule(s) defined\n", rules.len());
    for r in rules {
        match r.rule_type.as_str() {
            "no_dependency" => {
                let from = r.from.as_deref().unwrap_or("?");
                let targets = to_list(r).join(", ");
                println!("  no_dependency  {}", r.id);
                println!("    {from} → cannot depend on: {targets}");
                if let Some(reason) = &r.reason {
                    println!("    \"{reason}\"");
                }
                println!();
            }
            "boundary" => {
                println!("  boundary  {}", r.id);
                if !r.modules.is_empty() {
                    println!("    modules: {}", r.modules.join(", "));
                }
                if let Some(c) = &r.constraint {
                    println!("    constraint: {c}");
                }
                if let Some(reason) = &r.reason {
                    println!("    \"{reason}\"");
                }
                println!();
            }
            "restrict_callers_to" => {
                println!("  restrict_callers_to  {}", r.id);
                if let Some(m) = &r.module {
                    println!("    protected: {m}");
                }
                if !r.allowed.is_empty() {
                    println!("    allowed callers: {}", r.allowed.join(", "));
                }
                if let Some(reason) = &r.reason {
                    println!("    \"{reason}\"");
                }
                println!();
            }
            other => {
                println!("  {other}  {}", r.id);
                if let Some(reason) = &r.reason {
                    println!("    \"{reason}\"");
                }
                println!();
            }
        }
    }

    Ok(())
}
