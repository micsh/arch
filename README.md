# arch

Architecture-as-code for any codebase. Define, query, and validate your project structure using YAML — designed for humans and AI agents alike.

## Why

AI coding agents are powerful but architecturally blind. They can write code, but they don't know where it belongs, what depends on what, or which boundaries to respect. `arch` gives any codebase a machine-readable architecture definition that agents read before making changes and update after.

For humans, it replaces scattered tribal knowledge with a queryable, version-controlled source of truth.

## Install

```bash
cargo install arch
```

Or download a pre-built binary from [GitHub Releases](https://github.com/micsh/arch/releases).

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

# Which modules need attention (validates YAML + checks coverage)?
arch stale
```

## What It Creates

```
your-project/
  architecture.yaml              # System map: containers, rules, guidance
  architecture/
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
| `arch owns <concept>` | Find which module owns a concept |
| `arch stale` | Check architecture health: validate YAML integrity + find unmapped files |
| `arch drift` | Compare declared dependencies against actual code imports |
| `arch fitness` | Validate architectural rules against actual code |

## Designed for AI

The `guidance:` block in `architecture.yaml` is injected into AI agent prompts automatically. It tells agents:

1. **Before coding** — read the relevant container YAML for ownership and boundaries
2. **After coding** — update the YAML if ownership or dependencies changed
3. **Cross-cutting changes** — check `stories.yaml` to understand impact

The YAML schema is intentionally simple — no special query language, no database, no build step. Agents read YAML files directly, the same way they read code.

### For AI-powered teams

When multiple AI agents work on a codebase, `arch` provides:

- **Ownership routing** — which agent/coder owns which modules
- **Boundary enforcement** — what each module should and shouldn't do
- **Impact analysis** — stories show which modules a change affects
- **Self-describing** — the YAML defines its own usage instructions

## YAML Schema Reference

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
| `boundary` | `module`/`modules`, `constraint` | Enforces a constraint on what a module can do |

### Story Fields

| Field | Required | Description |
|-------|----------|-------------|
| `id` | ✅ | Unique story identifier |
| `description` | ✅ | What happens in this flow |
| `flow` | ✅ | Ordered list of `container/module` steps |

## Language Support for `arch init`

`arch init` detects project type and scans accordingly:

| Language | Detection | What it scans |
|----------|-----------|---------------|
| Rust | `Cargo.toml` | Workspace members, `mod.rs` files |
| .NET | `*.sln`, `*.csproj`, `*.fsproj` | Projects, namespaces, references |
| JavaScript/TypeScript | `package.json` | `src/` structure, imports |
| Python | `pyproject.toml`, `setup.py` | Package directories, `__init__.py` |
| Go | `go.mod` | Package directories |
| Generic | Fallback | Directory structure as containers |

## Contributing

Contributions welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

MIT
