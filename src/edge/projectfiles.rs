#![expect(
    clippy::disallowed_methods,
    reason = "this is the only module that lists the project's folders and resolves their links"
)]

use std::fs;
use std::path::Path;

// The names of the files at the top of the project, and those under `.config/` written as
// `.config/<name>`: where hook managers keep their config.
pub(crate) fn config_names(top: &Path) -> Vec<String> {
    let names = |dir: &Path, prefix: &str| -> Vec<String> {
        let Ok(entries) = fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut found: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| !kind.is_dir()))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .map(|name| format!("{prefix}{name}"))
            .collect();
        found.sort();
        found
    };
    let mut all = names(top, "");
    all.extend(names(&top.join(".config"), ".config/"));
    all
}

// Whether the real place of `path` (the nearest part of it that exists, links followed) is in
// one of the roots: a link inside the project that leads out of it does not count as inside.
pub(crate) fn inside_project(path: &Path, roots: &[&Path]) -> bool {
    let Some(existing) = path.ancestors().find(|part| fs::metadata(part).is_ok()) else {
        return false;
    };
    let Ok(real) = fs::canonicalize(existing) else {
        return false;
    };
    roots
        .iter()
        .filter_map(|root| fs::canonicalize(root).ok())
        .any(|root| real.starts_with(root))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;
    use crate::edge::testing::scratch;

    #[test]
    fn names_at_the_top_and_under_config_and_no_folders() {
        let top = scratch("config-names");
        fs::write(top.join("lefthook.yml"), "x").expect("a file");
        fs::create_dir(top.join("sub")).expect("a folder");
        fs::create_dir(top.join(".config")).expect("a folder");
        fs::write(top.join(".config").join("lefthook.yml"), "x").expect("a file");
        assert_eq!(
            config_names(&top),
            vec!["lefthook.yml".to_owned(), ".config/lefthook.yml".to_owned()]
        );
        assert_eq!(config_names(&top.join("missing")), Vec::<String>::new());
        fs::remove_dir_all(&top).ok();
    }

    #[test]
    fn a_link_that_leaves_the_project_is_outside() {
        let base = scratch("inside-project");
        let top = base.join("top");
        let elsewhere = base.join("elsewhere");
        fs::create_dir_all(top.join("hooks")).expect("a folder");
        fs::create_dir_all(&elsewhere).expect("a folder");
        symlink(&elsewhere, top.join("out")).expect("a link");
        let roots = [top.as_path()];
        assert!(inside_project(&top.join("hooks").join("pre-push"), &roots));
        assert!(inside_project(
            &top.join("not").join("yet").join("there"),
            &roots
        ));
        assert!(!inside_project(&top.join("out").join("pre-push"), &roots));
        assert!(!inside_project(
            &top.join("..").join("elsewhere").join("x"),
            &roots
        ));
        assert!(!inside_project(&elsewhere.join("x"), &roots));
        fs::remove_dir_all(&base).ok();
    }
}
