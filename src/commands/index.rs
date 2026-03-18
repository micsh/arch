use crate::context::ArchContext;
use crate::llmcode;
use crate::lmindex;

/// Compile and write arch.index.json + llmcode.index.json.
///
/// With --json: print both indexes to stdout as a combined object.
/// Without --json: write files to architecture/generated/ and print confirmation.
pub fn run(json: bool) -> Result<(), String> {
    let ctx = ArchContext::load()?;
    let mod_index = ctx.build_index();

    // Discover and parse all .llmcode files
    let llmcode_paths = llmcode::discover_llmcode_files(&ctx.root);
    let mut parsed_files = Vec::new();
    for path in &llmcode_paths {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Could not read '{}': {e}", path.display()))?;
        let parsed = llmcode::parse_llmcode_file(path, &content)?;
        parsed_files.push(parsed);
    }

    let arch_index = lmindex::build_arch_index(&ctx);
    let llmcode_index = lmindex::build_llmcode_index(&ctx, &parsed_files, &mod_index);

    if json {
        let combined = serde_json::json!({
            "arch_index": arch_index,
            "llmcode_index": llmcode_index,
        });
        crate::context::print_json(&combined)?;
        return Ok(());
    }

    // Write to architecture/generated/
    let generated_dir = ctx.root.join("architecture").join("generated");
    std::fs::create_dir_all(&generated_dir)
        .map_err(|e| format!("Could not create '{}': {e}", generated_dir.display()))?;

    let arch_path = generated_dir.join("arch.index.json");
    let llmcode_path = generated_dir.join("llmcode.index.json");

    write_json(&arch_path, &arch_index)?;
    write_json(&llmcode_path, &llmcode_index)?;

    println!(
        "✅ Indexes written:\n  {}\n  {}",
        arch_path.display(),
        llmcode_path.display()
    );

    Ok(())
}

fn write_json(path: &std::path::Path, value: &serde_json::Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| format!("JSON serialization failed: {e}"))?;
    std::fs::write(path, text)
        .map_err(|e| format!("Could not write '{}': {e}", path.display()))
}
