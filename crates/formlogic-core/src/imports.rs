use std::collections::HashSet;

/// Resolve `import "path"` / `import './path'` statements by inlining file contents.
///
/// The `resolver` callback takes a path string (as written in the import) and returns
/// the file contents, or `None` if the file can't be found.
///
/// Handles recursive imports and prevents circular dependencies via a visited set.
/// Strips UTF-8 BOM from imported files automatically.
pub fn resolve_imports<F>(source: &str, resolver: &F) -> String
where
    F: Fn(&str) -> Option<String>,
{
    let mut visited = HashSet::new();
    resolve_imports_inner(strip_bom(source), resolver, &mut visited)
}

/// Strip UTF-8 BOM (byte order mark) if present at the start of a string.
fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{FEFF}').unwrap_or(s)
}

fn resolve_imports_inner<F>(source: &str, resolver: &F, visited: &mut HashSet<String>) -> String
where
    F: Fn(&str) -> Option<String>,
{
    let mut output = String::with_capacity(source.len());

    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(path) = parse_import_line(trimmed) {
            if visited.contains(&path) {
                // Skip circular/duplicate import
                output.push('\n');
                continue;
            }
            visited.insert(path.clone());

            if let Some(contents) = resolver(&path) {
                // Recursively resolve imports in the imported file (strip BOM)
                let resolved = resolve_imports_inner(strip_bom(&contents), resolver, visited);
                output.push_str(&resolved);
                output.push('\n');
            } else {
                // File not found — skip the import line
                output.push('\n');
            }
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }

    output
}

/// Parse a line like `import "./cards.logic"` or `import './cards.logic'`
/// and return the path string, or None if it's not an import statement.
fn parse_import_line(line: &str) -> Option<String> {
    let rest = line.strip_prefix("import")?;

    // Must be followed by whitespace
    if !rest.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let rest = rest.trim();

    // Extract quoted string
    let (quote, rest) = if rest.starts_with('"') {
        ('"', &rest[1..])
    } else if rest.starts_with('\'') {
        ('\'', &rest[1..])
    } else {
        return None;
    };

    let end = rest.find(quote)?;
    let path = &rest[..end];

    // After the closing quote, only whitespace or semicolons allowed
    let after = rest[end + 1..].trim();
    if !after.is_empty() && after != ";" {
        return None;
    }

    Some(path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_import_double_quotes() {
        assert_eq!(
            parse_import_line(r#"import "./cards.logic""#),
            Some("./cards.logic".to_string())
        );
    }

    #[test]
    fn test_parse_import_single_quotes() {
        assert_eq!(
            parse_import_line("import './cards.logic'"),
            Some("./cards.logic".to_string())
        );
    }

    #[test]
    fn test_parse_import_with_semicolon() {
        assert_eq!(
            parse_import_line(r#"import "./cards.logic";"#),
            Some("./cards.logic".to_string())
        );
    }

    #[test]
    fn test_parse_not_import() {
        assert_eq!(parse_import_line("let x = 5"), None);
        assert_eq!(parse_import_line("// import './foo'"), None);
        // import with from (ES module style) should NOT match
        assert_eq!(parse_import_line(r#"import { foo } from "./bar""#), None);
    }

    #[test]
    fn test_resolve_basic() {
        let source = "let x = 1\nimport \"./a.logic\"\nlet y = 2\n";
        let result = resolve_imports(source, &|path| {
            if path == "./a.logic" {
                Some("let a = 10".to_string())
            } else {
                None
            }
        });
        assert!(result.contains("let x = 1"));
        assert!(result.contains("let a = 10"));
        assert!(result.contains("let y = 2"));
        assert!(!result.contains("import"));
    }

    #[test]
    fn test_resolve_nested() {
        let source = "import \"./a.logic\"\n";
        let result = resolve_imports(source, &|path| match path {
            "./a.logic" => Some("import \"./b.logic\"\nlet a = 1".to_string()),
            "./b.logic" => Some("let b = 2".to_string()),
            _ => None,
        });
        assert!(result.contains("let b = 2"));
        assert!(result.contains("let a = 1"));
    }

    #[test]
    fn test_resolve_circular() {
        let source = "import \"./a.logic\"\n";
        let result = resolve_imports(source, &|path| match path {
            "./a.logic" => Some("import \"./b.logic\"\nlet a = 1".to_string()),
            "./b.logic" => Some("import \"./a.logic\"\nlet b = 2".to_string()),
            _ => None,
        });
        // Should not infinite loop; both files included once
        assert!(result.contains("let a = 1"));
        assert!(result.contains("let b = 2"));
    }
}
