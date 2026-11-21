#![expect(
    clippy::disallowed_methods,
    clippy::use_debug,
    reason = "a build script reads the tree and the environment; {:?} writes escaped string literals"
)]

use std::fmt::Write as _;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

// `data` is named even when it does not exist yet, so the first file added to it is noticed.
const RERUN: [&str; 2] = ["cargo:rerun-if-changed=src", "cargo:rerun-if-changed=data"];

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

// What would silence the effect lints: their own name anywhere, or a group that holds them
// (`clippy::style`, `clippy::all`, `warnings`) inside an `allow` or `expect` attribute.
const GROUPS: [&str; 3] = ["clippy::style", "clippy::all", "warnings"];

fn suppression(text: &str) -> Option<&'static str> {
    if text.contains("disallowed_") {
        return Some("disallowed_");
    }
    ["allow(", "expect("]
        .into_iter()
        .flat_map(|marker| text.match_indices(marker))
        .find_map(|(at, _)| {
            let attribute = text.get(at..)?.split(']').next()?;
            GROUPS.into_iter().find(|group| attribute.contains(group))
        })
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
        if !matches!(folder, Folder::Edge) {
            let text =
                fs::read_to_string(&path).map_err(|why| format!("cannot read {shown}: {why}"))?;
            if let Some(found) = suppression(&text) {
                return Err(format!(
                    "{shown}: files in src/pure and src/commands must not suppress the effect lints (found '{found}'); move the effect into src/edge"
                ));
            }
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
            "    Command {{ name: {name:?}, hidden: commands::{stem}::HIDDEN, run: commands::{stem}::run }},"
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
    for directive in RERUN {
        println!("{directive}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edge::testing::scratch;

    #[test]
    fn effect_lints_are_not_suppressed_by_name_or_by_a_group_that_holds_them() {
        for text in [
            "#[expect(clippy::disallowed_methods, reason = \"x\")]",
            "#![expect(clippy::style, reason = \"x\")]",
            "#[expect(clippy::all, reason = \"x\")]",
            "#[allow(clippy::style)]",
            "#[expect(warnings, reason = \"x\")]",
            "#[cfg_attr(test, expect(clippy::all, reason = \"x\"))]",
            "#[expect(dead_code, clippy::style, reason = \"x\")]",
        ] {
            assert!(suppression(text).is_some(), "{text}");
        }
    }

    #[test]
    fn other_suppressions_and_plain_text_pass() {
        for text in [
            "#[expect(clippy::cast_possible_truncation, reason = \"checked\")]",
            "#[expect(clippy::struct_field_names, reason = \"the key\")]",
            "// clippy::style is the group of the effect lints\nfn f() {}",
            "fn f() {}",
        ] {
            assert_eq!(suppression(text), None, "{text}");
        }
    }

    fn folder_with(name: &str, folder: &str, text: &str) -> PathBuf {
        let root = scratch(name);
        let dir = root.join("src").join(folder);
        fs::create_dir_all(&dir).expect("makes the folder");
        fs::write(dir.join("x.rs"), text).expect("writes");
        root
    }

    #[test]
    fn a_pure_or_command_file_that_suppresses_the_effect_lints_stops_the_build() {
        let text = "#[expect(clippy::style, reason = \"x\")]\nfn f() {}\n";
        for (name, folder, kind) in [
            ("build-pure", "pure", Folder::Pure),
            ("build-commands", "commands", Folder::Commands),
        ] {
            let root = folder_with(name, folder, text);
            let refusal = modules(&root, kind).err().expect("refused");
            assert!(refusal.contains("found 'clippy::style'"), "{refusal}");
            assert!(refusal.contains("x.rs"), "{refusal}");
            fs::remove_dir_all(&root).ok();
        }
    }

    #[test]
    fn an_edge_file_may_suppress_them() {
        let root = folder_with(
            "build-edge",
            "edge",
            "#![expect(clippy::disallowed_methods, reason = \"x\")]\n",
        );
        assert_eq!(modules(&root, Folder::Edge).map(|found| found.len()), Ok(1));
        fs::remove_dir_all(&root).ok();
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
    fn data_is_watched_even_before_it_exists() {
        assert!(RERUN.contains(&"cargo:rerun-if-changed=data"));
        assert!(RERUN.contains(&"cargo:rerun-if-changed=src"));
    }
}
