mod commands;
mod schema;
mod scanner;
mod imports;
mod resolve;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "arch", version, about = "Architecture-as-code for any codebase")]
struct Cli {
    /// Output results as JSON instead of human-readable text
    #[arg(long, global = true)]
    json: bool,

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
    /// Verify story flows against actual import connections
    Stories,
    /// Generate Mermaid diagrams from architecture YAML
    Mermaid {
        /// Generate story flow diagrams instead of container diagram
        #[arg(long)]
        stories: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let json = cli.json;

    let result = match cli.command {
        Commands::Init => commands::init::run(),
        Commands::Validate => commands::validate::run(json),
        Commands::Coverage => commands::coverage::run(json),
        Commands::Owns { concept } => commands::owns::run(&concept, json),
        Commands::Stale => commands::stale::run(json),
        Commands::Drift => commands::drift::run(json),
        Commands::Fitness => commands::fitness::run(json),
        Commands::Stories => commands::stories::run(json),
        Commands::Mermaid { stories } => commands::mermaid::run(stories),
    };

    if let Err(e) = result {
        if json {
            let err = serde_json::json!({ "error": e });
            println!("{}", serde_json::to_string_pretty(&err).unwrap());
        } else {
            eprintln!("Error: {e}");
        }
        std::process::exit(1);
    }
}
