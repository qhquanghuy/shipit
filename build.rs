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
        if !matches!(folder, Folder::Edge) {
            let text =
                fs::read_to_string(&path).map_err(|why| format!("cannot read {shown}: {why}"))?;
            if text.contains("disallowed_") {
                return Err(format!(
                    "{shown}: files in src/pure and src/commands must not suppress the effect lints (found 'disallowed_'); move the effect into src/edge"
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
    println!("cargo:rerun-if-changed=src");
    if root.join("data").exists() {
        println!("cargo:rerun-if-changed=data");
    }
    Ok(())
}
