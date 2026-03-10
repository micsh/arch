use super::{validate, coverage, drift};
use crate::context::print_json;

pub fn run(json: bool) -> Result<(), String> {
    let val = validate::check()?;
    let cov = coverage::check()?;
    let dft = drift::check()?;

    let forbidden: Vec<&drift::DriftItem> = dft.items.iter().filter(|d| d.kind == "forbidden").collect();
    let undeclared: Vec<&drift::DriftItem> = dft.items.iter().filter(|d| d.kind == "undeclared").collect();

    if json {
        let output = serde_json::json!({
            "valid": val.errors.is_empty(),
            "containers": val.container_count,
            "errors": val.errors,
            "warnings": val.warnings,
            "unmapped": cov.unmapped,
            "unmapped_count": cov.unmapped.len(),
            "drift_scanned": dft.scanned_count,
            "drift_issues": dft.items.len(),
            "drift_forbidden": forbidden,
            "drift_undeclared": undeclared,
        });
        print_json(&output)?;
        if !val.errors.is_empty() {
            return Err(format!("{} error(s)", val.errors.len()));
        }
        if !forbidden.is_empty() {
            return Err(format!("{} forbidden violation(s)", forbidden.len()));
        }
        return Ok(());
    }

    // — Text output —
    let has_val_issues = !val.errors.is_empty() || !val.warnings.is_empty();
    let has_unmapped = !cov.unmapped.is_empty();
    let has_drift = !dft.items.is_empty();

    if !has_val_issues && !has_unmapped && !has_drift {
        println!(
            "✅ Architecture is up to date ({} containers, all files mapped, no drift)",
            val.container_count
        );
        return Ok(());
    }

    if has_val_issues {
        println!("[validate]");
        for e in &val.errors {
            println!("  ❌ {e}");
        }
        for w in &val.warnings {
            println!("  ⚠️  {w}");
        }
    }

    if has_unmapped {
        println!("[coverage]");
        for f in &cov.unmapped {
            println!("  📂 {f}");
        }
    }

    if has_drift {
        println!("[drift]");
        for d in &forbidden {
            println!("  🚫 {} ({}:{})", d.module_id, d.file, d.line_number);
            println!("      import: {} → {}", d.import_raw, d.target_module);
        }
        for d in &undeclared {
            println!("  ⚠️  {} ({}:{})", d.module_id, d.file, d.line_number);
            println!("      import: {} → {}", d.import_raw, d.target_module);
        }
    }

    let total = val.errors.len() + val.warnings.len() + cov.unmapped.len() + dft.items.len();
    println!("\n⏰ {} issue(s) found", total);

    if !val.errors.is_empty() {
        Err(format!("{} error(s)", val.errors.len()))
    } else if !forbidden.is_empty() {
        Err(format!("{} forbidden violation(s)", forbidden.len()))
    } else {
        Ok(())
    }
}
