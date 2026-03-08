use std::path::Path;

/// An import extracted from a source file.
#[derive(Debug, Clone)]
pub struct Import {
    pub raw: String,
    pub line_number: usize,
}

/// Extract imports from a source file based on its extension.
pub fn extract_imports(path: &Path, content: &str) -> Vec<Import> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    match ext {
        "fs" | "fsx" => extract_fsharp(content),
        "cs" => extract_csharp(content),
        "rs" => extract_rust(content),
        "ts" | "tsx" | "js" | "jsx" => extract_typescript(content),
        "py" => extract_python(content),
        "go" => extract_go(content),
        _ => vec![],
    }
}

fn extract_fsharp(content: &str) -> Vec<Import> {
    content
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let trimmed = line.trim();
            if trimmed.starts_with("open ") && !trimmed.starts_with("open type ") {
                let module = trimmed.strip_prefix("open ")?.trim();
                Some(Import {
                    raw: module.to_string(),
                    line_number: i + 1,
                })
            } else if trimmed.starts_with("open type ") {
                let module = trimmed.strip_prefix("open type ")?.trim();
                Some(Import {
                    raw: module.to_string(),
                    line_number: i + 1,
                })
            } else {
                None
            }
        })
        .collect()
}

fn extract_csharp(content: &str) -> Vec<Import> {
    content
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let trimmed = line.trim();
            if trimmed.starts_with("using ") && trimmed.ends_with(';') && !trimmed.contains('(') {
                let ns = trimmed
                    .strip_prefix("using ")?
                    .trim_end_matches(';')
                    .trim();
                // Skip aliases like "using X = Y"
                if ns.contains('=') {
                    let rhs = ns.split('=').nth(1)?.trim();
                    Some(Import {
                        raw: rhs.to_string(),
                        line_number: i + 1,
                    })
                } else if ns.starts_with("static ") {
                    let static_ns = ns.strip_prefix("static ")?.trim();
                    Some(Import {
                        raw: static_ns.to_string(),
                        line_number: i + 1,
                    })
                } else {
                    Some(Import {
                        raw: ns.to_string(),
                        line_number: i + 1,
                    })
                }
            } else {
                None
            }
        })
        .collect()
}

fn extract_rust(content: &str) -> Vec<Import> {
    let mut in_test_cfg = false;
    let mut in_raw_string = false;

    content
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let trimmed = line.trim();

            // Track raw string boundaries
            if trimmed.contains("r#\"") {
                in_raw_string = true;
            }
            if in_raw_string {
                if trimmed.contains("\"#") {
                    in_raw_string = false;
                }
                return None;
            }

            // Stop at test modules
            if trimmed == "#[cfg(test)]" {
                in_test_cfg = true;
                return None;
            }
            if in_test_cfg {
                return None;
            }

            if trimmed.starts_with("use ") || trimmed.starts_with("mod ") {
                let keyword = if trimmed.starts_with("use ") {
                    "use "
                } else {
                    "mod "
                };
                let path = trimmed
                    .strip_prefix(keyword)?
                    .trim_end_matches(';')
                    .trim();
                // Skip pub mod re-exports
                if path.starts_with("pub ") {
                    return None;
                }
                Some(Import {
                    raw: path.to_string(),
                    line_number: i + 1,
                })
            } else {
                None
            }
        })
        .collect()
}

fn extract_typescript(content: &str) -> Vec<Import> {
    content
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let trimmed = line.trim();
            // import ... from '...' or import ... from "..."
            if trimmed.contains("from ") && (trimmed.contains("import ") || trimmed.contains("import{")) {
                let from_part = trimmed.split("from ").last()?;
                let module = from_part
                    .trim()
                    .trim_end_matches(';')
                    .trim_matches('\'')
                    .trim_matches('"');
                Some(Import {
                    raw: module.to_string(),
                    line_number: i + 1,
                })
            } else if trimmed.starts_with("require(") || trimmed.contains("require(") {
                // const x = require('...')
                let start = trimmed.find("require(")? + 8;
                let rest = &trimmed[start..];
                let end = rest.find(')')?;
                let module = rest[..end].trim_matches('\'').trim_matches('"');
                Some(Import {
                    raw: module.to_string(),
                    line_number: i + 1,
                })
            } else {
                None
            }
        })
        .collect()
}

fn extract_python(content: &str) -> Vec<Import> {
    content
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let trimmed = line.trim();
            if trimmed.starts_with("import ") {
                let module = trimmed.strip_prefix("import ")?.split(" as ").next()?.trim();
                Some(Import {
                    raw: module.to_string(),
                    line_number: i + 1,
                })
            } else if trimmed.starts_with("from ") && trimmed.contains(" import ") {
                let module = trimmed
                    .strip_prefix("from ")?
                    .split(" import ")
                    .next()?
                    .trim();
                Some(Import {
                    raw: module.to_string(),
                    line_number: i + 1,
                })
            } else {
                None
            }
        })
        .collect()
}

fn extract_go(content: &str) -> Vec<Import> {
    let mut results = Vec::new();
    let mut in_import_block = false;

    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("import (") {
            in_import_block = true;
            continue;
        }
        if in_import_block {
            if trimmed == ")" {
                in_import_block = false;
                continue;
            }
            let module = trimmed.trim_matches('"').trim();
            if !module.is_empty() {
                // Handle aliased imports: alias "path"
                let actual = if module.contains('"') {
                    module.split('"').nth(1).unwrap_or(module)
                } else {
                    module
                };
                results.push(Import {
                    raw: actual.to_string(),
                    line_number: i + 1,
                });
            }
        } else if trimmed.starts_with("import \"") {
            let module = trimmed
                .strip_prefix("import \"")
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or("");
            if !module.is_empty() {
                results.push(Import {
                    raw: module.to_string(),
                    line_number: i + 1,
                });
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_fsharp_imports() {
        let content = r#"
namespace AITeam.Core

open AITeam.Boards
open AITeam.Protocols.Types
open type AITeam.Core.AgentId
open System.Collections.Generic
"#;
        let imports = extract_imports(&PathBuf::from("test.fs"), content);
        assert_eq!(imports.len(), 4);
        assert_eq!(imports[0].raw, "AITeam.Boards");
        assert_eq!(imports[1].raw, "AITeam.Protocols.Types");
        assert_eq!(imports[2].raw, "AITeam.Core.AgentId");
        assert_eq!(imports[3].raw, "System.Collections.Generic");
    }

    #[test]
    fn test_csharp_imports() {
        let content = r#"
using System;
using Microsoft.AspNetCore.Mvc;
using static System.Console;
using Alias = MyNamespace.MyClass;
"#;
        let imports = extract_imports(&PathBuf::from("test.cs"), content);
        assert_eq!(imports.len(), 4);
        assert_eq!(imports[0].raw, "System");
        assert_eq!(imports[1].raw, "Microsoft.AspNetCore.Mvc");
        assert_eq!(imports[2].raw, "System.Console");
        assert_eq!(imports[3].raw, "MyNamespace.MyClass");
    }

    #[test]
    fn test_rust_imports() {
        let content = r#"
use crate::schema;
use super::validate;
mod helpers;
use std::path::Path;
"#;
        let imports = extract_imports(&PathBuf::from("test.rs"), content);
        assert_eq!(imports.len(), 4);
        assert_eq!(imports[0].raw, "crate::schema");
        assert_eq!(imports[1].raw, "super::validate");
        assert_eq!(imports[2].raw, "helpers");
        assert_eq!(imports[3].raw, "std::path::Path");
    }

    #[test]
    fn test_python_imports() {
        let content = r#"
import os
import json as j
from pathlib import Path
from .utils import helper
"#;
        let imports = extract_imports(&PathBuf::from("test.py"), content);
        assert_eq!(imports.len(), 4);
        assert_eq!(imports[0].raw, "os");
        assert_eq!(imports[1].raw, "json");
        assert_eq!(imports[2].raw, "pathlib");
        assert_eq!(imports[3].raw, ".utils");
    }

    #[test]
    fn test_typescript_imports() {
        let content = r#"
import { Router } from 'express';
import React from "react";
const fs = require('fs');
"#;
        let imports = extract_imports(&PathBuf::from("test.ts"), content);
        assert_eq!(imports.len(), 3);
        assert_eq!(imports[0].raw, "express");
        assert_eq!(imports[1].raw, "react");
        assert_eq!(imports[2].raw, "fs");
    }

    #[test]
    fn test_go_imports() {
        let content = r#"
package main

import "fmt"

import (
	"os"
	"net/http"
	log "github.com/sirupsen/logrus"
)
"#;
        let imports = extract_imports(&PathBuf::from("test.go"), content);
        assert_eq!(imports.len(), 4);
        assert_eq!(imports[0].raw, "fmt");
        assert_eq!(imports[1].raw, "os");
        assert_eq!(imports[2].raw, "net/http");
        assert_eq!(imports[3].raw, "github.com/sirupsen/logrus");
    }

    #[test]
    fn test_fsharp_skips_comments_and_strings() {
        let content = r#"
open AITeam.Core
// open AITeam.ShouldNotMatch
let x = "open AITeam.AlsoNot"
open AITeam.Boards
"#;
        let imports = extract_imports(&PathBuf::from("test.fs"), content);
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].raw, "AITeam.Core");
        assert_eq!(imports[1].raw, "AITeam.Boards");
    }

    #[test]
    fn test_rust_skips_pub_mod() {
        let content = r#"
pub mod commands;
mod schema;
use crate::imports;
"#;
        let imports = extract_imports(&PathBuf::from("test.rs"), content);
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].raw, "schema");
        assert_eq!(imports[1].raw, "crate::imports");
    }
}
