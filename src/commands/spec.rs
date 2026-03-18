use crate::schema::{ARCH_SPEC, LLMCODE_SPEC};

/// Display grammar specs understood by this binary.
///
/// `--llmcode` — print the full llmcode grammar spec.
/// `--arch`    — print the full .arch grammar spec.
/// (no flag)   — print usage listing both flags.
pub fn run(llmcode: bool, arch: bool) -> Result<(), String> {
    match (llmcode, arch) {
        (true, false) => print!("{LLMCODE_SPEC}"),
        (false, true) => print!("{ARCH_SPEC}"),
        (true, true) => {
            print!("{LLMCODE_SPEC}");
            println!();
            print!("{ARCH_SPEC}");
        }
        (false, false) => {
            println!("arch spec — display grammar specs");
            println!();
            println!("  --llmcode   Full llmcode grammar spec (field reference, syntax, examples)");
            println!("  --arch      Full .arch grammar spec (field reference, parsing rules, examples)");
            println!();
            println!("Example: arch spec --arch");
        }
    }
    Ok(())
}
