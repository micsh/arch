use super::{validate, coverage, drift, fitness};
use crate::context::{ArchContext, print_json};
use crate::llmcode;
use crate::lmindex;

pub fn run(json: bool) -> Result<(), String> {
    let ctx = ArchContext::load()?;
    let val = validate::check(&ctx)?;
    let cov = coverage::check(&ctx)?;
    let dft = drift::check(&ctx)?;
    let fit = fitness::check(&ctx)?;

    // Discover and parse .llmcode files for staleness check
    let llmcode_paths = llmcode::discover_llmcode_files(&ctx.root);
    let mut llmcode_files = Vec::new();
    for path in &llmcode_paths {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(parsed) = llmcode::parse_llmcode_file(path, &content) {
                llmcode_files.push(parsed);
            }
        }
    }
    let stale_blocks = lmindex::check_llmcode_staleness(&ctx, &llmcode_files);

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
            "fitness_passed": fit.passed,
            "fitness_failed": fit.failed,
            "fitness_manual": fit.manual,
            "llmcode_stale_count": stale_blocks.len(),
            "llmcode_stale": stale_blocks.iter().map(|s| serde_json::json!({
                "file": s.file,
                "reason": s.reason,
            })).collect::<Vec<_>>(),
        });
        print_json(&output)?;
        if !val.errors.is_empty() {
            return Err(format!("{} error(s)", val.errors.len()));
        }
        if !forbidden.is_empty() {
            return Err(format!("{} forbidden violation(s)", forbidden.len()));
        }
        if fit.failed > 0 {
            return Err(format!("{} fitness rule(s) failed", fit.failed));
        }
        return Ok(());
    }

    // — Text output —
    let has_val_issues = !val.errors.is_empty() || !val.warnings.is_empty();
    let has_unmapped = !cov.unmapped.is_empty();
    let has_drift = !dft.items.is_empty();
    let has_fitness_failures = fit.failed > 0;
    let has_stale_llmcode = !stale_blocks.is_empty();

    if !has_val_issues && !has_unmapped && !has_drift && !has_fitness_failures && !has_stale_llmcode {
        println!(
            "✅ Architecture is up to date ({} containers, all files mapped, no drift, {} fitness rules passed)",
            val.container_count, fit.passed
        );
        return Ok(());
    }

    if has_val_issues {
        println!("[validate]");
        for e in &val.errors { println!("  ❌ {e}"); }
        for w in &val.warnings { println!("  ⚠️  {w}"); }
    }

    if has_unmapped {
        println!("[coverage]");
        for f in &cov.unmapped { println!("  📂 {f}"); }
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

    if has_fitness_failures {
        println!("[fitness]");
        for r in fit.results.iter().filter(|r| !r.passed) {
            println!("  ❌ {} — FAILED", r.rule_id);
            for v in &r.violations { println!("      {v}"); }
        }
    }

    if has_stale_llmcode {
        println!("[llmcode]");
        for s in &stale_blocks {
            println!("  ⚠️  {} ({}) — {}", s.file, s.llmcode_path.display(), s.reason);
        }
    }

    let total = val.errors.len() + val.warnings.len() + cov.unmapped.len()
        + dft.items.len() + fit.failed + stale_blocks.len();
    println!("\n⏰ {} issue(s) found", total);

    if !val.errors.is_empty() {
        Err(format!("{} error(s)", val.errors.len()))
    } else if !forbidden.is_empty() {
        Err(format!("{} forbidden violation(s)", forbidden.len()))
    } else if fit.failed > 0 {
        Err(format!("{} fitness rule(s) failed", fit.failed))
    } else {
        Ok(())
    }
}
