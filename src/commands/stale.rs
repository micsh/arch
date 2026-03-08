use super::{validate, coverage};

pub fn run() -> Result<(), String> {
    let val = validate::check()?;
    let cov = coverage::check()?;

    let has_errors = !val.errors.is_empty();
    let has_warnings = !val.warnings.is_empty();
    let has_unmapped = !cov.unmapped.is_empty();

    if !has_errors && !has_warnings && !has_unmapped {
        println!("✅ Architecture is up to date ({} containers, all files mapped)", val.container_count);
        return Ok(());
    }

    // Validation errors
    for e in &val.errors {
        println!("❌ {e}");
    }
    for w in &val.warnings {
        println!("⚠️  {w}");
    }

    // Unmapped files
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
