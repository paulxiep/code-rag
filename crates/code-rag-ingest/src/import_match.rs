//! Language-aware import-path matching for tier-2 edge resolution.
//!
//! Answers one question: does a candidate file (repo-relative, forward-slash)
//! plausibly be the file an import statement refers to? Every rule here is
//! **anchored** — exact paths or `/`-boundary suffixes — with no substring
//! heuristics. A specifier that names an external package (stdlib, npm, a Go
//! domain path) matches nothing: third-party targets have no chunk in the
//! index, so a loose match could only be wrong.
//!
//! The language is taken from the *importing* file's extension, because the
//! specifier grammar is the importing language's. Candidates whose extension
//! doesn't belong to that language are rejected outright.
//!
//! Per-language rules (each documented on its function):
//! - Rust: `crate::`-anchored module paths, module-aware `super::`/`self::`
//!   resolution, cross-crate `<crate>::…` against `<crate dir>/src/…`.
//! - Python: dotted absolute paths, dot-relative resolution from the source
//!   file's package directory.
//! - TypeScript: `./`/`../` specifiers resolved against the source directory;
//!   bare/scoped specifiers are treated as external packages.
//! - Go: module-relative package-directory suffixes (≥2 segments); domain
//!   imports are external.
//!
//! History: this replaces an undocumented single-normalization matcher whose
//! unanchored `contains` fallback let `super::foo` match any `foo.rs`,
//! mis-tagged `from . import x` resolutions as tier 2, and silently never
//! matched TS relative or Go imports at all.

/// Does `candidate_path` satisfy the import `source_path` written in
/// `source_file`? All paths repo-relative with forward slashes.
pub fn import_matches(candidate_path: &str, source_path: &str, source_file: &str) -> bool {
    match extension(source_file) {
        "rs" => match_rust(candidate_path, source_path, source_file),
        "py" => match_python(candidate_path, source_path, source_file),
        "ts" | "tsx" => match_typescript(candidate_path, source_path, source_file),
        "go" => match_go(candidate_path, source_path),
        _ => false,
    }
}

/// Rust: the specifier is the `use` path minus the imported name (e.g.
/// `use crate::a::b::C;` arrives as `crate::a::b`).
///
/// - `crate::a::b` → candidate ends (at a `/` boundary) with `a/b.rs` or
///   `a/b/mod.rs`.
/// - `super::…` / `self::…` → resolved against the source *module*: a file
///   `dir/c.rs` is itself a child of the directory module, so one `super`
///   lands in `dir/` and each further `super` walks up; for `mod.rs` /
///   `lib.rs` / `main.rs` (whose module *is* the directory) `super` walks up
///   immediately. The result is compared exactly, not by suffix.
/// - Anything else is a foreign crate: matched only against a workspace
///   crate layout `<name>/src/…` with `_`/`-` interchangeable in the crate
///   dir (so `code_rag_types` finds `crates/code-rag-types/src/lib.rs`).
///   `std::…`/`serde::…` fail those anchors naturally — no blocklist needed.
fn match_rust(candidate: &str, spec: &str, source_file: &str) -> bool {
    if !candidate.ends_with(".rs") {
        return false;
    }
    if let Some(rest) = spec.strip_prefix("crate::") {
        let p = rest.replace("::", "/");
        return ends_with_at_slash(candidate, &format!("{p}.rs"))
            || ends_with_at_slash(candidate, &format!("{p}/mod.rs"));
    }
    if spec == "self"
        || spec == "super"
        || spec.starts_with("self::")
        || spec.starts_with("super::")
    {
        return match_rust_relative(candidate, spec, source_file).unwrap_or(false);
    }

    // Foreign crate path: `<crate>::rest…` against a workspace `<dir>/src/` layout.
    let mut segs = spec.split("::");
    let crate_seg = match segs.next() {
        Some(s) if !s.is_empty() => s,
        _ => return false,
    };
    let rest: Vec<&str> = segs.collect();
    for dir_name in [crate_seg.replace('_', "-"), crate_seg.to_string()] {
        let root = format!("{dir_name}/src");
        if rest.is_empty() {
            if ends_with_at_slash(candidate, &format!("{root}/lib.rs")) {
                return true;
            }
        } else {
            let p = rest.join("/");
            if ends_with_at_slash(candidate, &format!("{root}/{p}.rs"))
                || ends_with_at_slash(candidate, &format!("{root}/{p}/mod.rs"))
            {
                return true;
            }
        }
    }
    false
}

/// `super::`/`self::` resolution. Returns None when the path walks off the
/// repo root (treated as no match).
fn match_rust_relative(candidate: &str, spec: &str, source_file: &str) -> Option<bool> {
    let dir = parent_dir(source_file)?;
    let file_is_dir_module = matches!(basename(source_file), "mod.rs" | "lib.rs" | "main.rs");

    let mut segs = spec.split("::").peekable();
    // `self` keeps the current module; each `super` moves to the parent module.
    // For a plain file `dir/c.rs` the current module's children live in
    // `dir/c/`; its parent (one `super`) is the directory module `dir/`.
    let mut module_dir: String = if file_is_dir_module {
        dir.to_string()
    } else {
        // current module = the file itself; children under dir/<stem>/
        let stem = basename(source_file).trim_end_matches(".rs");
        format!("{dir}/{stem}")
    };
    while let Some(&seg) = segs.peek() {
        match seg {
            "self" => {
                segs.next();
            }
            "super" => {
                segs.next();
                module_dir = match module_dir.rfind('/') {
                    Some(i) => module_dir[..i].to_string(),
                    None => return None, // walked off the repo root
                };
            }
            _ => break,
        }
    }
    let rest: Vec<&str> = segs.collect();
    if rest.is_empty() {
        // `use super::X` — X is defined in the module itself: its file is
        // either `module_dir.rs` or `module_dir/mod.rs`.
        return Some(
            candidate == format!("{module_dir}.rs") || candidate == format!("{module_dir}/mod.rs"),
        );
    }
    let p = rest.join("/");
    let expected = format!("{module_dir}/{p}");
    Some(candidate == format!("{expected}.rs") || candidate == format!("{expected}/mod.rs"))
}

/// Python: dotted module paths.
///
/// - Absolute `a.b` → candidate ends (at a `/` boundary) with `a/b.py` or
///   `a/b/__init__.py`.
/// - Relative `.x` / `..pkg.mod`: one leading dot is the source file's own
///   package directory, each extra dot walks up one; the remainder is then
///   resolved to an exact `…/x.py` / `…/x/__init__.py` path.
/// - Bare `.` / `..` (`from . import x`): the target is the resolved package
///   itself — its `__init__.py` or a module directly inside it. (The old
///   matcher normalized this to `/` and matched *every* file.)
fn match_python(candidate: &str, spec: &str, source_file: &str) -> bool {
    if !candidate.ends_with(".py") {
        return false;
    }
    let dots = spec.len() - spec.trim_start_matches('.').len();
    let rest = spec.trim_start_matches('.');
    if dots == 0 {
        let p = rest.replace('.', "/");
        return ends_with_at_slash(candidate, &format!("{p}.py"))
            || ends_with_at_slash(candidate, &format!("{p}/__init__.py"));
    }
    let Some(mut dir) = parent_dir(source_file) else {
        return false;
    };
    for _ in 1..dots {
        match parent_dir(dir) {
            Some(d) => dir = d,
            None => return false,
        }
    }
    if rest.is_empty() {
        return candidate == format!("{dir}/__init__.py") || parent_dir(candidate) == Some(dir);
    }
    let expected = format!("{dir}/{}", rest.replace('.', "/"));
    candidate == format!("{expected}.py") || candidate == format!("{expected}/__init__.py")
}

/// TypeScript: only relative specifiers (`./x`, `../x/y`) resolve — they are
/// joined to the source file's directory with `.`/`..` normalization and an
/// optional `.js`/`.jsx`/`.ts`/`.tsx` extension stripped, then compared
/// exactly against `{p}.ts(x)` / `{p}/index.ts(x)`. Bare or scoped
/// specifiers (`react`, `@scope/pkg`, `utils`) are external packages and
/// never match — in-repo path aliases are deliberately sacrificed rather
/// than risking substring matches.
fn match_typescript(candidate: &str, spec: &str, source_file: &str) -> bool {
    if !(candidate.ends_with(".ts") || candidate.ends_with(".tsx")) {
        return false;
    }
    if !(spec.starts_with("./") || spec.starts_with("../")) {
        return false;
    }
    let base = parent_dir(source_file).unwrap_or("");
    let Some(resolved) = resolve_relative(base, spec) else {
        return false;
    };
    let resolved = strip_ts_extension(&resolved);
    for ext in [".ts", ".tsx"] {
        if candidate == format!("{resolved}{ext}") || candidate == format!("{resolved}/index{ext}")
        {
            return true;
        }
    }
    false
}

/// Go: the specifier is a package (directory) path. Imports whose first
/// segment contains a `.` are domain paths (external module); single-segment
/// imports (`fmt`) are stdlib/external. Otherwise the module prefix is
/// unknown without go.mod, so leading segments are stripped one at a time and
/// the candidate's *directory* must end (at a `/` boundary) with a remainder
/// of at least two segments — `myapp/internal/store` matches any file in
/// `…/internal/store/`, but a bare `store` dir elsewhere never matches.
fn match_go(candidate: &str, spec: &str) -> bool {
    if !candidate.ends_with(".go") {
        return false;
    }
    let segs: Vec<&str> = spec.split('/').filter(|s| !s.is_empty()).collect();
    if segs.len() < 2 || segs[0].contains('.') {
        return false;
    }
    let Some(cand_dir) = parent_dir(candidate) else {
        return false;
    };
    for start in 0..=segs.len() - 2 {
        let suffix = segs[start..].join("/");
        if ends_with_at_slash(cand_dir, &suffix) {
            return true;
        }
    }
    false
}

// ---- shared path helpers (forward-slash, repo-relative) ----

fn extension(path: &str) -> &str {
    basename(path)
        .rsplit_once('.')
        .map(|(_, e)| e)
        .unwrap_or("")
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn parent_dir(path: &str) -> Option<&str> {
    path.rfind('/').map(|i| &path[..i])
}

/// `path` equals `suffix` or ends with `/suffix` — a suffix match that can
/// never bind in the middle of a path segment.
fn ends_with_at_slash(path: &str, suffix: &str) -> bool {
    path == suffix
        || path
            .strip_suffix(suffix)
            .is_some_and(|head| head.ends_with('/'))
}

/// Join a `./`/`../` specifier onto `base_dir`, normalizing `.`/`..`
/// segments. Returns None if the path would escape the repo root.
fn resolve_relative(base_dir: &str, spec: &str) -> Option<String> {
    let mut parts: Vec<&str> = if base_dir.is_empty() {
        Vec::new()
    } else {
        base_dir.split('/').collect()
    };
    for seg in spec.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                // popping past the repo root propagates None via `?`
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

fn strip_ts_extension(path: &str) -> &str {
    for ext in [".tsx", ".ts", ".jsx", ".js"] {
        if let Some(stripped) = path.strip_suffix(ext) {
            return stripped;
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Rust ----

    #[test]
    fn rust_crate_anchored() {
        assert!(import_matches(
            "code-rag/src/ingestion/parser.rs",
            "crate::ingestion::parser",
            "code-rag/src/other.rs"
        ));
        assert!(import_matches(
            "code-rag/src/ingestion/parser/mod.rs",
            "crate::ingestion::parser",
            "code-rag/src/other.rs"
        ));
        // Anchored: a different parent dir must not match.
        assert!(!import_matches(
            "code-rag/src/other/parser.rs",
            "crate::ingestion::parser",
            "code-rag/src/other.rs"
        ));
        // Boundary: `stion/parser.rs` inside a longer segment must not match.
        assert!(!import_matches(
            "code-rag/src/xingestion/parser.rs",
            "crate::ingestion::parser",
            "code-rag/src/other.rs"
        ));
    }

    #[test]
    fn rust_super_resolves_to_sibling_module_only() {
        // From p/src/languages/rust.rs, `use super::helper::X`: the module of
        // rust.rs has its children in languages/rust/, so one `super` is the
        // `languages` directory module → helper resolves to a sibling file.
        assert!(import_matches(
            "p/src/languages/helper.rs",
            "super::helper",
            "p/src/languages/rust.rs"
        ));
        // The old matcher's failure mode: same name in a faraway dir.
        assert!(!import_matches(
            "p/other/helper.rs",
            "super::helper",
            "p/src/languages/rust.rs"
        ));
        // From a mod.rs, super walks up immediately.
        assert!(import_matches(
            "p/src/helper.rs",
            "super::helper",
            "p/src/languages/mod.rs"
        ));
    }

    #[test]
    fn rust_self_is_child_module() {
        // dir module (mod.rs): self::x → sibling file in same dir.
        assert!(import_matches(
            "p/src/ingestion/reconcile.rs",
            "self::reconcile",
            "p/src/ingestion/mod.rs"
        ));
        // plain file c.rs: self::x → child at c/x.rs.
        assert!(import_matches("p/src/c/x.rs", "self::x", "p/src/c.rs"));
        assert!(!import_matches("p/src/x.rs", "self::x", "p/src/c.rs"));
    }

    #[test]
    fn rust_cross_crate_underscore_hyphen() {
        assert!(import_matches(
            "code-rag/crates/code-rag-types/src/lib.rs",
            "code_rag_types",
            "code-rag/crates/code-rag-store/src/vector_store.rs"
        ));
        assert!(import_matches(
            "code-rag/crates/code-rag-store/src/seams.rs",
            "code_rag_store::seams",
            "code-rag/crates/code-raptor/src/lib.rs"
        ));
        // std/serde fail the /src anchors naturally.
        assert!(!import_matches(
            "p/src/collections.rs",
            "std::collections",
            "p/src/a.rs"
        ));
    }

    // ---- Python ----

    #[test]
    fn python_absolute_dotted() {
        assert!(import_matches(
            "app/utils/helper.py",
            "utils.helper",
            "app/main.py"
        ));
        assert!(import_matches(
            "app/utils/helper/__init__.py",
            "utils.helper",
            "app/main.py"
        ));
        assert!(!import_matches(
            "app/other/helper.py",
            "utils.helper",
            "app/main.py"
        ));
    }

    #[test]
    fn python_relative_dots() {
        // .helpers from pkg/mod.py → pkg/helpers.py, not any helpers.py.
        assert!(import_matches(
            "app/pkg/helpers.py",
            ".helpers",
            "app/pkg/mod.py"
        ));
        assert!(!import_matches(
            "app/other/helpers.py",
            ".helpers",
            "app/pkg/mod.py"
        ));
        // ..sub.mod walks up one package.
        assert!(import_matches(
            "app/sub/mod.py",
            "..sub.mod",
            "app/pkg/leaf.py"
        ));
    }

    #[test]
    fn python_bare_dot_is_bounded() {
        // from . import x → only the package itself, never everything.
        assert!(import_matches("app/pkg/__init__.py", ".", "app/pkg/mod.py"));
        assert!(import_matches("app/pkg/sibling.py", ".", "app/pkg/mod.py"));
        assert!(!import_matches("app/other/thing.py", ".", "app/pkg/mod.py"));
    }

    // ---- TypeScript ----

    #[test]
    fn ts_relative_resolves() {
        assert!(import_matches(
            "web/src/utils/format.ts",
            "./utils/format",
            "web/src/App.tsx"
        ));
        assert!(import_matches(
            "web/src/utils/index.ts",
            "./utils",
            "web/src/App.tsx"
        ));
        assert!(import_matches(
            "web/src/lib/api.tsx",
            "../lib/api",
            "web/src/pages/Home.tsx"
        ));
        // .js specifier resolves to the .ts source.
        assert!(import_matches(
            "web/src/store.ts",
            "./store.js",
            "web/src/App.tsx"
        ));
    }

    #[test]
    fn ts_bare_and_scoped_are_external() {
        assert!(!import_matches(
            "web/src/react.ts",
            "react",
            "web/src/App.tsx"
        ));
        assert!(!import_matches(
            "web/src/scope/pkg.ts",
            "@scope/pkg",
            "web/src/App.tsx"
        ));
        // No substring matching: 'utils' vs my_utils_old.
        assert!(!import_matches(
            "web/src/my_utils_old/x.ts",
            "utils",
            "web/src/App.tsx"
        ));
    }

    #[test]
    fn ts_escaping_root_never_matches() {
        assert!(!import_matches("x.ts", "../../../x", "web/App.tsx"));
    }

    // ---- Go ----

    #[test]
    fn go_module_relative_package() {
        assert!(import_matches(
            "myapp/internal/store/store.go",
            "myapp/internal/store",
            "myapp/cmd/main.go"
        ));
        // Module prefix stripped: portfolio paths carry a project prefix the
        // import path lacks — the ≥2-segment suffix still anchors.
        assert!(import_matches(
            "proj/app/internal/store/db.go",
            "app/internal/store",
            "proj/app/cmd/main.go"
        ));
    }

    #[test]
    fn go_external_and_single_segment_rejected() {
        // Single-segment → stdlib/external.
        assert!(!import_matches("p/fmt/x.go", "fmt", "p/main.go"));
        // Domain path (dot in first segment) → external module.
        assert!(!import_matches(
            "p/github/com/sirupsen/logrus/log.go",
            "github.com/sirupsen/logrus",
            "p/main.go"
        ));
        // ≥2-segment remainder required: a bare `store` dir elsewhere must not
        // match `aaa/bbb/store`.
        assert!(!import_matches(
            "p/store/x.go",
            "aaa/bbb/store",
            "p/main.go"
        ));
    }

    // ---- cross-language guards ----

    #[test]
    fn candidate_extension_must_match_language() {
        assert!(!import_matches(
            "app/utils/helper.py",
            "crate::utils::helper",
            "p/src/a.rs"
        ));
        assert!(!import_matches(
            "p/src/utils/helper.rs",
            "utils.helper",
            "app/main.py"
        ));
    }
}
