# arch

Architecture-as-code for any codebase. Define, query, and validate your project structure using YAML — designed for humans and AI agents alike.

## Why

AI coding agents are powerful but architecturally blind. They can write code, but they don't know where it belongs, what depends on what, or which boundaries to respect. `arch` gives any codebase a machine-readable architecture definition that agents read before making changes and update after.

For humans, it replaces scattered tribal knowledge with a queryable, version-controlled source of truth.

## Install

**Quick install (recommended):**

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/micsh/arch/main/install.sh | bash

# Windows (PowerShell)
irm https://raw.githubusercontent.com/micsh/arch/main/install.ps1 | iex
```

**Or build from source:**

```bash
cargo install arch
```

Pre-built binaries for all platforms are also available from [GitHub Releases](https://github.com/micsh/arch/releases).

## Quick Start

```bash
# In your project root — scaffold architecture from project structure
arch init

# Check everything is consistent
arch validate

# Who owns the "authentication" concept?
arch owns authentication

# Which source files aren't mapped to any module?
arch coverage

# Full health check (validate + coverage combined)
arch stale

# Do actual imports match declared dependencies?
arch drift

# Do architectural rules hold against the real code?
arch fitness

# Generate Mermaid container diagram
arch mermaid

# Any command with JSON output (for tooling / agent consumption)
arch drift --json
```

## What It Creates

```
your-project/
  architecture/
    architecture.yaml            # System map: containers, rules, guidance
    stories.yaml                 # Cross-cutting flows
    backend.yaml                 # Per-container module details
    frontend.yaml
    ...
```

### `architecture.yaml` — The Map

```yaml
guidance: |
  Before making code changes, read the relevant container YAML.
  After changes, update ownership and dependencies if they changed.

system:
  name: MyProject
  description: A web application with API and frontend
  ignore:
    - "tests/**"
    - "**/*.test.ts"
    - "scripts/**"

containers:
  - id: backend
    path: src/backend
    description: REST API and business logic
    depends_on: []

  - id: frontend
    path: src/frontend
    description: React UI
    depends_on: [backend]

rules:
  - id: backend-independence
    type: no_dependency
    from: backend
    to: frontend
    reason: "Backend must not depend on frontend"
```

### `architecture/<container>.yaml` — The Details

```yaml
# backend.yaml
modules:
  - id: auth
    file: auth/mod.rs
    owns: [authentication, token-validation, session-management]
    depends_on: [backend/database]
    boundary: "Auth logic only — no direct DB queries, use database module"

  - id: database
    file: db/mod.rs
    owns: [connection-pool, migrations, query-execution]
    must_not_depend: [backend/auth]
    boundary: "Data access only — no business logic"
```

### `architecture/stories.yaml` — The Flows

```yaml
stories:
  - id: user-login
    description: User submits credentials → validated → token issued → stored
    flow:
      - frontend/login-page
      - backend/auth
      - backend/database
      - backend/auth           # token generation
```

## Commands

| Command | Description |
|---------|-------------|
| `arch init` | Scan project structure and generate initial architecture YAML |
| `arch validate` | Check YAML integrity: files exist, cross-refs valid, schema correct |
| `arch coverage` | List source files not mapped to any module |
| `arch owns <concept>` | Find which module owns a concept, file, or module ID |
| `arch stale` | Full health check: validate + coverage combined |
| `arch drift` | Compare declared dependencies against actual code imports |
| `arch fitness` | Validate architectural rules against actual code |
| `arch stories` | Verify story flows against actual import connections |
| `arch mermaid` | Generate Mermaid container dependency diagram |
| `arch mermaid --stories` | Generate Mermaid flowcharts from stories.yaml |

All commands except `init` and `mermaid` support `--json` for structured output.

### `arch drift` — Dependency Drift Detection

Parses actual `import`/`open`/`use` statements from source files, resolves them to architecture modules, and compares against declared `depends_on` and `must_not_depend`:

```
$ arch drift
🚫 1 forbidden dependency violation(s):

  backend/database (db/mod.rs:3) → backend/auth via `use crate::auth`

⚠️  2 undeclared dependency(ies):

  backend/api (api/mod.rs:5) → frontend/shared via `use crate::shared`
  backend/api (api/mod.rs:8) → backend/metrics via `use crate::metrics`

📊 12 modules scanned, 3 issue(s) found
```

Supports: F#, C#, Rust, TypeScript/JavaScript, Python, Go.

### `arch fitness` — Rule Validation

Evaluates architecture rules from `architecture.yaml` against the actual codebase:

```
$ arch fitness
✅ backend-independence
✅ database-leaf
❌ api-isolation — FAILED
    backend/api → frontend/shared (forbidden: backend → frontend)
📋 types-purity (manual check) No function implementations — only type definitions

📊 4 rule(s): 2 passed, 1 failed, 1 manual
```

- **`no_dependency` rules** are checked against the real import graph — violations are errors
- **`boundary` rules** are prose constraints reported as manual-check items

### `arch stories` — Flow Verification

Validates that story flows in `stories.yaml` are backed by real import connections between modules:

```
$ arch stories
📖 trading-cycle — One complete trading cycle: fetch market data and account state...
  app/orchestrator → app/data  ✅
  app/data → app/indicators  ✅
  app/indicators → app/awareness  ✅
  app/awareness → app/prompts  ✅
  app/prompts → app/ai  ✅
  app/ai → app/execution  ✅
  ✅ 6/6 connections verified

📊 3/3 stories fully connected
```

For each consecutive pair (A→B) in a flow, stories checks that A imports from B **or** B imports from A (bidirectional — handles event-driven and callback patterns). Same-project modules in compiled languages (.NET, Rust) are considered implicitly connected.

### Directory-Aware Coverage

When a module's `file:` points to a recognized entry point (`__init__.py`, `mod.rs`, `index.ts`, etc.) or a project file (`.csproj`, `.fsproj`, `.vbproj`), all source files in that directory tree are automatically covered by that module. This means you only need one module per package/directory — use `owns:` for concepts, sub-modules for distinct boundary enforcement:

```yaml
# Python: one module covers the entire models/ package and subpackages
- id: models
  file: models/__init__.py
  owns: [market-snapshot, account-state, trade-decision, order-request]
  boundary: "Pure data structures only — no logic, no I/O"
  depends_on: []
```

### .NET Monorepo Pattern

For .NET solutions, use one container per `.csproj` project. Point the module's `file:` at the `.csproj` — arch will recursively scan all `.cs`/`.fs` files in the project directory for imports:

```yaml
# architecture.yaml
containers:
  - id: data-processing
    path: src/DataProcessing
    project: MyApp.DataProcessing
    description: Data pipeline and transformation
    depends_on: [common]

  - id: common
    path: src/Common
    project: MyApp.Common
    description: Shared utilities and types
    depends_on: []
```

```yaml
# data-processing.yaml
modules:
  - id: app
    file: DataProcessing.Application.csproj
    owns: [pipeline-orchestration, data-transforms]
    depends_on: [common/shared]
```

### `arch mermaid` — Diagram Generation

Generates Mermaid diagrams from your architecture YAML. Output is Mermaid text — paste into any Mermaid-compatible renderer (GitHub, VS Code, Mermaid Live Editor).

```bash
# Container-level dependency diagram with module subgraphs
arch mermaid

# Story flow diagrams
arch mermaid --stories
```

Container diagram example output:
```
graph LR
    backend["REST API\n(5 modules)"]
    frontend["React UI\n(3 modules)"]
    frontend --> backend
```

### `--json` Output

All commands (except `init` and `mermaid`) support `--json` for structured output. Useful for CI pipelines, VS Code extensions, MCP servers, and agent tool integrations:

```bash
$ arch drift --json
{
  "scanned": 12,
  "issues": 1,
  "forbidden": [
    {
      "module_id": "backend/database",
      "file": "db/mod.rs",
      "import_raw": "use crate::auth",
      "line_number": 3,
      "target_module": "backend/auth",
      "kind": "forbidden"
    }
  ],
  "undeclared": []
}

$ arch owns authentication --json
{
  "query": "authentication",
  "matches": [
    {
      "module": "backend/auth",
      "file": "auth/mod.rs",
      "owns": "authentication",
      "boundary": "Auth logic only — no direct DB queries"
    }
  ]
}
```

## Ignoring Files

Use the `ignore` field in `system:` to exclude files from `coverage` and `drift` checks. Patterns use glob syntax:

```yaml
system:
  name: MyProject
  ignore:
    - "tests/**"           # test directories
    - "**/*.test.ts"       # test files by naming convention
    - "scripts/**"         # build/dev scripts
    - "benchmarks/**"      # benchmark code
```

Ignored files won't be flagged as unmapped in `arch coverage` and won't be scanned for imports in `arch drift`.

## Designed for AI

The `guidance:` block in `architecture.yaml` provides instructions for AI agents working on the codebase. Teams can wire it into agent prompts however they like — the field is there as a convention. Typical guidance tells agents:

1. **Before coding** — read the relevant container YAML for ownership and boundaries
2. **After coding** — update the YAML if ownership or dependencies changed
3. **Cross-cutting changes** — check `stories.yaml` to understand impact

The YAML schema is intentionally simple — no special query language, no database, no build step. Agents read YAML files directly, the same way they read code.

### For AI-powered teams

When multiple AI agents work on a codebase, `arch` provides:

- **Ownership routing** — which agent/coder owns which modules
- **Boundary enforcement** — what each module should and shouldn't do
- **Drift detection** — real-time validation that code matches the architecture
- **Impact analysis** — stories show which modules a change affects
- **Self-describing** — the YAML defines its own usage instructions

## CI / Git Hooks

### GitHub Actions

Add architecture validation to your PR pipeline. Copy `examples/arch-ci.yml` to `.github/workflows/` or use this snippet:

```yaml
# .github/workflows/arch-ci.yml
name: Architecture Check
on:
  pull_request:
    branches: [main]
jobs:
  arch:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Download arch
        run: |
          curl -sL "https://github.com/micsh/arch/releases/latest/download/arch-linux-x64" -o arch
          chmod +x arch
      - run: ./arch validate
      - run: ./arch drift
      - run: ./arch fitness
```

### Pre-commit Hook

Validate architecture before every commit. Copy the hook from `examples/pre-commit`:

```bash
cp examples/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

The hook runs `arch validate` and `arch drift`, blocking the commit if violations are found. Skip temporarily with `git commit --no-verify`.

## YAML Schema Reference

### System Fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | ✅ | Project name |
| `description` | | What this system does |
| `ignore` | | Glob patterns for files to exclude from coverage and drift checks |

### Container Fields

| Field | Required | Description |
|-------|----------|-------------|
| `id` | ✅ | Unique identifier (used in cross-references) |
| `path` | ✅ | Relative path to source directory |
| `description` | ✅ | What this container does |
| `depends_on` | ✅ | List of container IDs this depends on |
| `project` | | Build system project name (e.g., crate, package, assembly) |
| `notes` | | Free-form notes |

### Module Fields

| Field | Required | Description |
|-------|----------|-------------|
| `id` | ✅ | Unique within container |
| `file` | ✅ | Relative path from container root |
| `owns` | ✅ | List of concepts this module is responsible for |
| `boundary` | | What this module should NOT do |
| `depends_on` | | List of `container/module` references |
| `must_not_depend` | | Explicit forbidden dependencies |
| `routes` | | Map of concept → target module (for wiring/orchestrator modules) |

### Rule Types

| Type | Fields | Description |
|------|--------|-------------|
| `no_dependency` | `from`, `to`, `reason` | Forbids dependency between modules/containers |
| `no_import_from` | `from`, `pattern`, `reason` | Forbids imports matching a glob pattern (e.g., `tests*`) |
| `boundary` | `module`/`modules`, `constraint` | Enforces a constraint on what a module can do |

Example `no_import_from` rule:

```yaml
- id: no-test-imports
  type: no_import_from
  from: mypackage           # container to check
  pattern: "tests*"         # matches against import path segments
  reason: "Production code must not import from test modules"
```

### Story Fields

| Field | Required | Description |
|-------|----------|-------------|
| `id` | ✅ | Unique story identifier |
| `description` | ✅ | What happens in this flow |
| `flow` | ✅ | Ordered list of `container/module` steps |

## Language Support

`arch drift` parses imports and `arch init` detects project type for these languages:

| Language | Detection | Import syntax |
|----------|-----------|---------------|
| Rust | `Cargo.toml` | `use crate::`, `mod`, `use super::` |
| F# | `*.fsproj` | `open Namespace.Module` |
| C# | `*.csproj` | `using Namespace;` |
| TypeScript/JS | `package.json` | `import ... from '...'`, `require('...')` |
| Python | `pyproject.toml`, `setup.py` | `import module`, `from module import ...` |
| Go | `go.mod` | `import "package"` |

## Contributing

Contributions welcome! Open an issue or submit a pull request.

## License

MIT
