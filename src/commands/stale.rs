use super::{validate, coverage};
use crate::context::print_json;

pub fn run(json: bool) -> Result<(), String> {
    let val = validate::check()?;
    let cov = coverage::check()?;

    if json {
        let output = serde_json::json!({
            "valid": val.errors.is_empty(),
            "containers": val.container_count,
            "errors": val.errors,
            "warnings": val.warnings,
            "unmapped": cov.unmapped,
            "unmapped_count": cov.unmapped.len(),
        });
        print_json(&output)?;
        if !val.errors.is_empty() {
            return Err(format!("{} error(s)", val.errors.len()));
        }
        return Ok(());
    }

    let has_errors = !val.errors.is_empty();
    let has_warnings = !val.warnings.is_empty();
    let has_unmapped = !cov.unmapped.is_empty();

    if !has_errors && !has_warnings && !has_unmapped {
        println!("✅ Architecture is up to date ({} containers, all files mapped)", val.container_count);
        return Ok(());
    }

    for e in &val.errors {
        println!("❌ {e}");
    }
    for w in &val.warnings {
        println!("⚠️  {w}");
    }

    if has_unmapped {
        println!("📂 {} unmapped source file(s):", cov.unmapped.len());
        for f in &cov.unmapped {
            println!("  {f}");
        }
    }

    let total = val.errors.len() + val.warnings.len() + cov.unmapped.len();
    println!("\n⏰ {} issue(s) — architecture YAML needs updating", total);

    if has_errors {
        Err(format!("{} error(s)", val.errors.len()))
    } else {
        Ok(())
    }
}
