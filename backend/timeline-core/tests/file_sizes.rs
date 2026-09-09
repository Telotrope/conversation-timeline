//! Static-analysis ratchet: no implementation file may exceed the 1,000-line
//! hard ceiling from CLAUDE.md's "Code organization and file size" rule.
//! `#[cfg(test)] mod tests { ... }` blocks are excluded from the count, since
//! that rule explicitly excludes test blocks — a file can be well within the
//! ceiling for implementation code while still carrying a thorough test
//! suite. New violations must fail this test; this backend starts from zero,
//! so the allowlist below starts (and should stay) empty.

use std::fs;
use std::path::{Path, PathBuf};

const HARD_CEILING: usize = 1000;

/// Paths (relative to the `backend/` workspace root) exempted from the
/// ceiling, with a reason. Empty on a greenfield backend — per CLAUDE.md:
/// "no grandfathered files, ever."
const ALLOWLIST: &[&str] = &[];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("timeline-core has a parent directory (the backend/ workspace root)")
        .to_path_buf()
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("reading dir {dir:?}: {e}")) {
        let entry = entry.expect("readable directory entry");
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "target" || name.starts_with('.') {
                continue;
            }
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Counts non-blank lines, excluding any `#[cfg(test)] mod tests { ... }`
/// block (brace-matched, so nested braces inside the test module don't
/// terminate the skip early).
fn implementation_line_count(source: &str) -> usize {
    let lines: Vec<&str> = source.lines().collect();
    let mut count = 0usize;
    let mut i = 0usize;
    while i < lines.len() {
        if lines[i].trim() == "#[cfg(test)]" {
            let mut j = i + 1;
            while j < lines.len() && !lines[j].contains("mod tests") {
                j += 1;
            }
            let mut depth = 0i32;
            let mut started = false;
            while j < lines.len() {
                for c in lines[j].chars() {
                    match c {
                        '{' => {
                            depth += 1;
                            started = true;
                        }
                        '}' => depth -= 1,
                        _ => {}
                    }
                }
                j += 1;
                if started && depth <= 0 {
                    break;
                }
            }
            i = j;
            continue;
        }
        if !lines[i].trim().is_empty() {
            count += 1;
        }
        i += 1;
    }
    count
}

#[test]
fn no_implementation_file_exceeds_the_line_ceiling() {
    let root = workspace_root();
    let mut files = Vec::new();
    collect_rs_files(&root, &mut files);
    assert!(
        !files.is_empty(),
        "expected to find at least one .rs file under {root:?}"
    );

    let mut violations = Vec::new();
    for path in files {
        let rel = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if ALLOWLIST.contains(&rel.as_str()) {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path:?}: {e}"));
        let lines = implementation_line_count(&source);
        if lines > HARD_CEILING {
            violations.push(format!(
                "{rel}: {lines} implementation lines (ceiling {HARD_CEILING})"
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "files exceeding the {HARD_CEILING}-line ceiling (see CLAUDE.md \"Code organization and file size\"):\n{}",
        violations.join("\n")
    );
}

#[cfg(test)]
mod self_tests {
    use super::implementation_line_count;

    #[test]
    fn excludes_test_module_but_counts_everything_else() {
        let source = "fn real_code() {}\n\n#[cfg(test)]\nmod tests {\n    fn a() {}\n    fn b() {\n        if true { }\n    }\n}\n";
        // Only "fn real_code() {}" is implementation.
        assert_eq!(implementation_line_count(source), 1);
    }

    #[test]
    fn counts_files_with_no_test_module_in_full() {
        let source = "fn a() {}\nfn b() {}\n";
        assert_eq!(implementation_line_count(source), 2);
    }

    #[test]
    fn blank_lines_are_not_counted() {
        let source = "fn a() {}\n\n\nfn b() {}\n";
        assert_eq!(implementation_line_count(source), 2);
    }
}
