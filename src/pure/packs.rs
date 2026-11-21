#![cfg(test)]

use crate::pure::markdown::{lines, section};
use crate::pure::markdown_parts::list_item;

fn pack(key: &str) -> &'static str {
    crate::DATA
        .iter()
        .find(|(path, _)| *path == key)
        .map(|(_, text)| *text)
        .expect("the pack is embedded")
}

fn items(text: &str, heading: &str) -> Vec<String> {
    let all = lines(text).expect("closed fences");
    section(&all, heading)
        .expect("one such heading")
        .iter()
        .filter_map(|line| list_item(line.text))
        .map(str::to_owned)
        .collect()
}

// The fifteen skill folders of Superpowers 7.0.2.
const SKILLS: [&str; 15] = [
    "brainstorming",
    "diagnosing-superpowers",
    "dispatching-parallel-agents",
    "executing-plans",
    "finishing-a-development-branch",
    "receiving-code-review",
    "requesting-code-review",
    "subagent-driven-development",
    "systematic-debugging",
    "test-driven-development",
    "using-git-worktrees",
    "using-superpowers",
    "verification-before-completion",
    "writing-plans",
    "writing-skills",
];

#[test]
fn superpowers_pack_names_every_skill() {
    let text = pack("packs/skill/superpowers.md");
    assert!(text.contains("7.0.2"));
    for skill in SKILLS {
        assert!(
            text.contains(&format!("`superpowers:{skill}`")),
            "no mention of superpowers:{skill}"
        );
    }
}

#[test]
fn superpowers_pack_has_overrides() {
    let text = pack("packs/skill/superpowers.md");
    assert_ne!(items(text, "## Phases").len(), 0);
    let overrides = items(text, "## Overrides");
    assert_ne!(overrides.len(), 0);
    for item in overrides {
        assert_eq!(item.split(" · ").count(), 4, "{item}");
        assert!(item.starts_with("`superpowers:"), "{item}");
    }
}

#[test]
fn coding_pack_covers_each_term() {
    let text = pack("packs/domain/coding.md").to_lowercase();
    for term in [
        "slice",
        "size",
        "recheck",
        "check commands",
        "security",
        "performance",
        "test drive",
        "ship",
    ] {
        assert!(text.contains(term), "no mention of {term}");
    }
}
