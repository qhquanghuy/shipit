#![expect(
    clippy::disallowed_methods,
    clippy::use_debug,
    reason = "a build script reads the tree and the environment; {:?} writes escaped string literals"
)]

use std::fmt::Write as _;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

// Names that cannot be a `mod` name.
const KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "async", "await", "dyn", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "typeof", "unsized", "virtual", "yield", "try", "gen",
];

#[derive(Clone, Copy)]
enum Folder {
    Pure,
    Edge,
    Commands,
}

impl Folder {
    const fn dir(self) -> &'static str {
        match self {
            Self::Pure => "pure",
            Self::Edge => "edge",
            Self::Commands => "commands",
        }
    }

    const fn digits(self) -> bool {
        match self {
            Self::Pure | Self::Edge => true,
            Self::Commands => false,
        }
    }

    // Only the edge may run effects: forbid (not deny) so that no `expect`, `allow`, macro or
    // `#[path]` file below can switch the lints off.
    const fn forbids_effects(self) -> bool {
        match self {
            Self::Pure | Self::Commands => true,
            Self::Edge => false,
        }
    }

    const fn rule(self) -> &'static str {
        if self.digits() {
            "^[a-z][a-z0-9_]*\\.rs$"
        } else {
            "^[a-z][a-z_]*\\.rs$"
        }
    }
}

struct Module {
    stem: String,
    path: String,
}

fn utf8<'a>(path: &'a Path, what: &str) -> Result<&'a str, String> {
    path.to_str()
        .ok_or_else(|| format!("{what} {} is not valid UTF-8", path.display()))
}

// A missing folder counts as an empty one: git and Nix drop empty folders.
fn entries(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let read = match fs::read_dir(dir) {
        Ok(read) => read,
        Err(why) if why.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(why) => return Err(format!("cannot read {}: {why}", dir.display())),
    };
    let mut paths = Vec::new();
    for entry in read {
        let entry = entry.map_err(|why| format!("cannot read {}: {why}", dir.display()))?;
        paths.push(entry.path());
    }
    paths.sort();
    Ok(paths)
}

fn name_matches(name: &str, folder: Folder) -> bool {
    let Some(stem) = name.strip_suffix(".rs") else {
        return false;
    };
    let mut chars = stem.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars
            .all(|c| c.is_ascii_lowercase() || c == '_' || (folder.digits() && c.is_ascii_digit()))
}

fn modules(root: &Path, folder: Folder) -> Result<Vec<Module>, String> {
    let dir = root.join("src").join(folder.dir());
    let mut found = Vec::new();
    for path in entries(&dir)? {
        let shown = path.display();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("{shown}: the file name is not valid UTF-8"))?;
        let meta = fs::metadata(&path).map_err(|why| format!("cannot read {shown}: {why}"))?;
        if meta.is_dir() {
            return Err(format!(
                "{shown} is a folder; the folders src/pure, src/edge and src/commands are flat"
            ));
        }
        if !name_matches(name, folder) {
            return Err(format!("{shown}: the name must match {}", folder.rule()));
        }
        let stem = name.strip_suffix(".rs").unwrap_or(name);
        if KEYWORDS.contains(&stem) {
            return Err(format!(
                "{shown}: '{stem}' is a Rust keyword, not a module name"
            ));
        }
        found.push(Module {
            stem: stem.to_owned(),
            path: utf8(&path, "path")?.to_owned(),
        });
    }
    Ok(found)
}

fn walk(dir: &Path, prefix: &str, found: &mut Vec<(String, String)>) -> Result<(), String> {
    for path in entries(dir)? {
        let shown = path.display();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("{shown}: the file name is not valid UTF-8"))?;
        let key = format!("{prefix}{name}");
        let meta = fs::metadata(&path).map_err(|why| format!("cannot read {shown}: {why}"))?;
        if meta.is_dir() {
            walk(&path, &format!("{key}/"), found)?;
        } else {
            let bytes = fs::read(&path).map_err(|why| format!("cannot read {shown}: {why}"))?;
            if String::from_utf8(bytes).is_err() {
                return Err(format!(
                    "{shown}: a data file must be UTF-8 text (it is embedded as a string)"
                ));
            }
            found.push((key, utf8(&path, "path")?.to_owned()));
        }
    }
    Ok(())
}

fn render_modules(out: &mut String, folder: Folder, found: &[Module]) -> Result<(), String> {
    let fail = |why: std::fmt::Error| why.to_string();
    if folder.forbids_effects() {
        writeln!(
            out,
            "#[forbid(clippy::disallowed_methods, clippy::disallowed_types)]"
        )
        .map_err(fail)?;
    }
    writeln!(out, "pub(crate) mod {} {{", folder.dir()).map_err(fail)?;
    for module in found {
        writeln!(
            out,
            "    #[path = {:?}]\n    pub(crate) mod {};",
            module.path, module.stem
        )
        .map_err(fail)?;
    }
    writeln!(out, "}}").map_err(fail)
}

fn render(root: &Path) -> Result<String, String> {
    let fail = |why: std::fmt::Error| why.to_string();
    let mut out = String::new();
    for folder in [Folder::Pure, Folder::Edge] {
        render_modules(&mut out, folder, &modules(root, folder)?)?;
    }
    let commands = modules(root, Folder::Commands)?;
    render_modules(&mut out, Folder::Commands, &commands)?;

    out.push_str("pub(crate) const COMMANDS: &[Command] = &[\n");
    let mut sorted: Vec<(String, &Module)> = commands
        .iter()
        .map(|module| (module.stem.replace('_', "-"), module))
        .collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, module) in sorted {
        let stem = &module.stem;
        writeln!(
            out,
            "    Command {{ name: {name:?}, hidden: commands::{stem}::HIDDEN, needs_setup: commands::{stem}::NEEDS_SETUP, usage: commands::{stem}::USAGE, summary: commands::{stem}::SUMMARY, exits: commands::{stem}::EXITS, run: commands::{stem}::run }},"
        )
        .map_err(fail)?;
    }
    out.push_str("];\n");

    let mut data = Vec::new();
    walk(&root.join("data"), "", &mut data)?;
    if !data.is_empty() {
        out.push_str("pub(crate) static DATA: &[(&str, &str)] = &[\n");
        for (key, path) in data {
            writeln!(out, "    ({key:?}, include_str!({path:?})),").map_err(fail)?;
        }
        out.push_str("];\n");
    }
    Ok(out)
}

// `data` is watched only when it exists: a path that does not exist makes cargo rebuild the crate
// on every run. The first file added to it comes with the source that reads it, which changes
// `src`, and that reruns this script.
fn rerun_directives(root: &Path) -> Vec<&'static str> {
    let mut directives = vec!["cargo:rerun-if-changed=src"];
    if root.join("data").exists() {
        directives.push("cargo:rerun-if-changed=data");
    }
    directives
}

// Only when the rules are tested as a module of the crate: a build script may print.
#[cfg_attr(
    test,
    expect(
        clippy::print_stdout,
        reason = "cargo reads the directives from stdout"
    )
)]
fn main() -> Result<(), String> {
    let root = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .ok_or("CARGO_MANIFEST_DIR is not set")?;
    let out_dir = std::env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or("OUT_DIR is not set")?;
    utf8(&root, "the source folder")?;
    let text = render(&root)?;
    fs::write(out_dir.join("modules.rs"), text)
        .map_err(|why| format!("cannot write modules.rs: {why}"))?;
    for directive in rerun_directives(&root) {
        println!("{directive}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::edge::testing::scratch;

    fn rendered(folder: Folder, stems: &[String]) -> String {
        let found: Vec<Module> = stems
            .iter()
            .map(|stem| Module {
                stem: stem.clone(),
                path: format!("/x/{stem}.rs"),
            })
            .collect();
        let mut out = String::new();
        render_modules(&mut out, folder, &found).expect("renders");
        out
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn only_the_edge_may_switch_the_effect_lints_off(
            stems in proptest::collection::vec("[a-z]{1,6}", 0..4),
        ) {
            let forbid = "#[forbid(clippy::disallowed_methods, clippy::disallowed_types)]\npub(crate) mod";
            for (folder, forbidden) in [
                (Folder::Pure, true),
                (Folder::Commands, true),
                (Folder::Edge, false),
            ] {
                prop_assert_eq!(rendered(folder, &stems).starts_with(forbid), forbidden);
            }
        }
    }

    #[test]
    fn clippy_refuses_the_effects_a_review_found_missing() {
        let clippy = include_str!("clippy.toml");
        for path in [
            "std::fs::exists",
            "std::fs::hard_link",
            "std::fs::soft_link",
            "std::fs::set_permissions",
            "std::os::unix::fs::symlink",
        ] {
            assert!(
                clippy.contains(&format!("{{ path = \"{path}\", ")),
                "{path}"
            );
        }
    }

    #[test]
    fn data_is_watched_once_it_exists() {
        let root = scratch("build-data");
        assert_eq!(rerun_directives(&root), ["cargo:rerun-if-changed=src"]);
        fs::create_dir_all(root.join("data")).expect("makes data");
        assert_eq!(
            rerun_directives(&root),
            ["cargo:rerun-if-changed=src", "cargo:rerun-if-changed=data"]
        );
        fs::remove_dir_all(&root).ok();
    }

    // clippy on a probe crate whose src/pure holds `source`, with the modules.rs this script
    // generates for it; None where clippy is not installed (the Nix test check has none).
    #[expect(
        clippy::disallowed_types,
        reason = "the test runs clippy on a probe crate"
    )]
    fn clippy_on(name: &str, source: &str) -> Option<(bool, String)> {
        use std::process::Command;

        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let version = Command::new(&cargo).args(["clippy", "--version"]).output();
        if !version.is_ok_and(|output| output.status.success()) {
            return None;
        }
        let root = scratch(name);
        fs::create_dir_all(root.join("src").join("pure")).expect("makes the folder");
        fs::write(root.join("src").join("pure").join("probe.rs"), source).expect("writes");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[workspace]\n",
        )
        .expect("writes");
        fs::write(root.join("clippy.toml"), include_str!("clippy.toml")).expect("writes");
        fs::write(
            root.join("src").join("main.rs"),
            "include!(\"modules.rs\");\nstruct Command {\n    name: &'static str,\n    hidden: bool,\n    needs_setup: bool,\n    usage: &'static str,\n    summary: &'static str,\n    exits: [&'static str; 4],\n    run: fn(),\n}\nfn main() {}\n",
        )
        .expect("writes");
        fs::write(
            root.join("src").join("modules.rs"),
            render(&root).expect("renders"),
        )
        .expect("writes");
        let output = Command::new(&cargo)
            .args(["clippy", "--quiet", "--manifest-path"])
            .arg(root.join("Cargo.toml"))
            .output()
            .expect("runs clippy");
        fs::remove_dir_all(&root).ok();
        Some((
            output.status.success(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }

    #[test]
    fn a_pure_file_cannot_run_an_effect_or_switch_the_lint_off() {
        let call = "std::fs::exists(\"x\").unwrap_or(false)";
        let cases = [
            (
                "plain",
                format!("pub(crate) fn f() -> bool {{ {call} }}"),
                "disallowed method",
            ),
            (
                "group",
                format!(
                    "#[expect(clippy::style, reason = \"x\")]\npub(crate) fn f() -> bool {{ {call} }}"
                ),
                "E0453",
            ),
            (
                "warnings",
                format!("#[expect(warnings)]\npub(crate) fn f() -> bool {{ {call} }}"),
                "disallowed method",
            ),
            (
                "macro",
                format!(
                    "macro_rules! quiet {{\n    ($g:ident, $f:item) => {{\n        #[expect(clippy::$g, reason = \"x\")]\n        $f\n    }};\n}}\nquiet!(style, pub(crate) fn f() -> bool {{ {call} }});"
                ),
                "E0453",
            ),
        ];
        for (name, source, wanted) in cases {
            let Some((ok, stderr)) = clippy_on(&format!("build-forbid-{name}"), &source) else {
                return;
            };
            assert!(!ok && stderr.contains(wanted), "{name}: {stderr}");
        }
        let (ok, stderr) = clippy_on("build-forbid-control", "pub(crate) fn f() {}")
            .expect("clippy was there a moment ago");
        assert!(ok, "{stderr}");
    }
}
