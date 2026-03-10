use crate::context::{ArchContext, print_json};
use crate::schema::Rule;

/// Extract the `to` field as a list of strings (handles both single string and sequence).
fn to_list(rule: &Rule) -> Vec<String> {
    match &rule.to {
        Some(serde_yaml::Value::String(s)) => vec![s.clone()],
        Some(serde_yaml::Value::Sequence(seq)) => seq
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        _ => vec![],
    }
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
    // Rules where this module is the `from` (what it cannot depend on)
    let from_rules: Vec<&Rule> = rules
        .iter()
        .filter(|r| r.rule_type == "no_dependency" && r.from.as_deref() == Some(module_name))
        .collect();

    // Rules where this module appears in `to` (what cannot depend on it)
    let to_rules: Vec<&Rule> = rules
        .iter()
        .filter(|r| r.rule_type == "no_dependency" && to_list(r).iter().any(|t| t == module_name))
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
        print_json(&serde_json::json!({
            "module": module_name,
            "cannot_depend_on": cannot_depend_on,
            "forbidden_dependents": forbidden_dependents,
        }))?;
        return Ok(());
    }

    if from_rules.is_empty() && to_rules.is_empty() {
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
