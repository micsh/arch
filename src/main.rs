mod commands;
mod schema;
mod scanner;
mod imports;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "arch", version, about = "Architecture-as-code for any codebase")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scan project structure and generate initial architecture YAML
    Init,
    /// Check YAML integrity: files exist, cross-refs valid, schema correct
    Validate,
    /// List source files not mapped to any module
    Coverage,
    /// Find which module owns a concept
    Owns {
        /// The concept to look up
        concept: String,
    },
    /// Check architecture health: validate YAML integrity + find unmapped source files
    Stale,
    /// Compare declared dependencies against actual code imports
    Drift,
    /// Validate architectural rules against actual code
    Fitness,
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init => commands::init::run(),
        Commands::Validate => commands::validate::run(),
        Commands::Coverage => commands::coverage::run(),
        Commands::Owns { concept } => commands::owns::run(&concept),
        Commands::Stale => commands::stale::run(),
        Commands::Drift => commands::drift::run(),
        Commands::Fitness => commands::fitness::run(),
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
