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

// Every skill has an override: a skill with none would run every default as written.
#[test]
fn every_skill_has_an_override() {
    let overrides = items(pack("packs/skill/superpowers.md"), "## Overrides");
    for skill in SKILLS {
        let name = format!("`superpowers:{skill}`");
        assert!(
            overrides.iter().any(|item| item
                .split(" · ")
                .next()
                .is_some_and(|head| head.contains(&name))),
            "no override for {skill}"
        );
    }
}

// The files of Superpowers 7.0.2 that the pack cites, by their path under `skills/`, with their
// line counts; a new citation adds its file here after counting its lines.
const CITED_FILES: [(&str, usize); 24] = [
    ("brainstorming/SKILL.md", 316),
    ("brainstorming/visual-companion.md", 299),
    ("diagnosing-superpowers/SKILL.md", 120),
    ("dispatching-parallel-agents/SKILL.md", 167),
    ("executing-plans/SKILL.md", 323),
    ("finishing-a-development-branch/SKILL.md", 225),
    ("receiving-code-review/SKILL.md", 205),
    ("requesting-code-review/SKILL.md", 95),
    ("requesting-code-review/code-reviewer.md", 198),
    ("subagent-driven-development/SKILL.md", 339),
    ("subagent-driven-development/implementer-prompt.md", 154),
    ("subagent-driven-development/references/final-review.md", 49),
    ("subagent-driven-development/references/fix-loop.md", 77),
    (
        "subagent-driven-development/references/handling-reports.md",
        15,
    ),
    ("subagent-driven-development/references/preflight.md", 26),
    ("subagent-driven-development/task-reviewer-prompt.md", 209),
    ("systematic-debugging/SKILL.md", 283),
    ("test-driven-development/SKILL.md", 330),
    ("using-git-worktrees/SKILL.md", 167),
    ("using-superpowers/SKILL.md", 65),
    ("using-superpowers/references/claude-code-tools.md", 29),
    ("verification-before-completion/SKILL.md", 120),
    ("writing-plans/SKILL.md", 204),
    ("writing-skills/SKILL.md", 399),
];

struct Cited {
    file: String,
    first: usize,
    last: usize,
}

// The head of an override names each skill as `superpowers:<skill>` followed by
// `(<file>:<lines>; <file>:<lines>)`, where lines are `a` or `a-b`, joined by `, `. A file in
// a code span is the same file: a file whose name the lint refuses (a harness name or a rejected
// term) may be cited only so.
fn citations(item: &str) -> Vec<Cited> {
    let head = item.split(" · ").next().unwrap_or_default();
    let mut found = Vec::new();
    for part in head.split("`superpowers:").skip(1) {
        let (skill, rest) = part.split_once('`').expect("a closed skill name");
        let (inside, _) = rest
            .strip_prefix(" (")
            .and_then(|rest| rest.split_once(')'))
            .unwrap_or_else(|| panic!("no citation after {skill}: {item}"));
        for cite in inside.split("; ") {
            let (path, lines) = cite
                .split_once(':')
                .unwrap_or_else(|| panic!("no lines in {cite}: {item}"));
            for range in lines.split(", ") {
                let (first, last) = range.split_once('-').unwrap_or((range, range));
                found.push(Cited {
                    file: format!("{skill}/{}", path.trim_matches('`')),
                    first: first
                        .parse()
                        .unwrap_or_else(|_why| panic!("{range}: {item}")),
                    last: last
                        .parse()
                        .unwrap_or_else(|_why| panic!("{range}: {item}")),
                });
            }
        }
    }
    found
}

#[test]
fn every_override_cites_lines_that_exist() {
    let overrides = items(pack("packs/skill/superpowers.md"), "## Overrides");
    let mut cited = Vec::new();
    for item in &overrides {
        let found = citations(item);
        assert_ne!(found.len(), 0, "{item}");
        for cite in found {
            let (_, count) = CITED_FILES
                .iter()
                .find(|(file, _)| *file == cite.file)
                .unwrap_or_else(|| panic!("{} is not a known file: {item}", cite.file));
            assert!(
                (1..=cite.last).contains(&cite.first) && cite.last <= *count,
                "{}:{}-{} is not inside its {count} lines: {item}",
                cite.file,
                cite.first,
                cite.last
            );
            cited.push(cite.file);
        }
    }
    for (file, _) in CITED_FILES {
        assert!(
            cited.iter().any(|name| name == file),
            "{file} is never cited"
        );
    }
}

// A range as wide as a file, or a file cut into adjacent ranges, would hide which lines an
// override is about: the reviews found items cited that way with every other test passing.
#[test]
fn no_override_cites_a_file_whole() {
    let overrides = items(pack("packs/skill/superpowers.md"), "## Overrides");
    for item in &overrides {
        let found = citations(item);
        for cite in &found {
            assert!(
                cite.last.saturating_sub(cite.first) < 60,
                "{}:{}-{} is wider than 60 lines: {item}",
                cite.file,
                cite.first,
                cite.last
            );
            let total: usize = found
                .iter()
                .filter(|other| other.file == cite.file)
                .map(|other| other.last.saturating_sub(other.first).saturating_add(1))
                .sum();
            assert!(
                total <= 120,
                "{} is cited for {total} lines in one item: {item}",
                cite.file
            );
        }
    }
}

// The places the informed review of the proof step superpowers-overrides-check (2026-11-22) and the
// later reviews of the fix rounds found with no override or with a wrong citation, one line of each.
const FOUND_BY_THE_REVIEWS: [(&str, usize); 59] = [
    ("subagent-driven-development/references/final-review.md", 12),
    ("subagent-driven-development/references/final-review.md", 18),
    ("subagent-driven-development/references/final-review.md", 29),
    ("subagent-driven-development/SKILL.md", 150),
    ("executing-plans/SKILL.md", 263),
    ("executing-plans/SKILL.md", 285),
    ("executing-plans/SKILL.md", 289),
    ("executing-plans/SKILL.md", 32),
    ("executing-plans/SKILL.md", 201),
    ("executing-plans/SKILL.md", 129),
    ("subagent-driven-development/references/fix-loop.md", 9),
    ("executing-plans/SKILL.md", 274),
    ("requesting-code-review/SKILL.md", 45),
    ("receiving-code-review/SKILL.md", 44),
    ("receiving-code-review/SKILL.md", 83),
    ("receiving-code-review/SKILL.md", 115),
    ("systematic-debugging/SKILL.md", 175),
    ("test-driven-development/SKILL.md", 319),
    ("writing-plans/SKILL.md", 135),
    ("brainstorming/SKILL.md", 23),
    ("brainstorming/SKILL.md", 276),
    ("brainstorming/SKILL.md", 282),
    ("using-git-worktrees/SKILL.md", 55),
    ("using-git-worktrees/SKILL.md", 100),
    ("using-git-worktrees/SKILL.md", 114),
    ("finishing-a-development-branch/SKILL.md", 134),
    (
        "subagent-driven-development/references/handling-reports.md",
        13,
    ),
    ("subagent-driven-development/references/preflight.md", 21),
    ("brainstorming/visual-companion.md", 58),
    ("writing-skills/SKILL.md", 385),
    ("using-superpowers/references/claude-code-tools.md", 20),
    ("requesting-code-review/code-reviewer.md", 108),
    ("brainstorming/SKILL.md", 106),
    ("using-superpowers/SKILL.md", 20),
    ("writing-plans/SKILL.md", 21),
    ("requesting-code-review/code-reviewer.md", 45),
    ("writing-plans/SKILL.md", 181),
    ("writing-plans/SKILL.md", 198),
    ("subagent-driven-development/references/fix-loop.md", 14),
    ("subagent-driven-development/task-reviewer-prompt.md", 153),
    (
        "subagent-driven-development/references/handling-reports.md",
        10,
    ),
    ("brainstorming/SKILL.md", 263),
    ("brainstorming/SKILL.md", 302),
    ("diagnosing-superpowers/SKILL.md", 57),
    ("requesting-code-review/SKILL.md", 14),
    ("subagent-driven-development/SKILL.md", 105),
    ("executing-plans/SKILL.md", 112),
    ("finishing-a-development-branch/SKILL.md", 48),
    ("subagent-driven-development/SKILL.md", 96),
    ("subagent-driven-development/references/final-review.md", 44),
    ("executing-plans/SKILL.md", 302),
    ("subagent-driven-development/implementer-prompt.md", 124),
    ("subagent-driven-development/SKILL.md", 283),
    ("writing-skills/SKILL.md", 12),
    ("brainstorming/visual-companion.md", 115),
    ("brainstorming/visual-companion.md", 275),
    ("diagnosing-superpowers/SKILL.md", 52),
    ("diagnosing-superpowers/SKILL.md", 64),
    ("diagnosing-superpowers/SKILL.md", 101),
];

#[test]
fn the_places_the_proof_review_found_are_overridden() {
    let overrides = items(pack("packs/skill/superpowers.md"), "## Overrides");
    let cited: Vec<Cited> = overrides
        .iter()
        .map(String::as_str)
        .flat_map(citations)
        .collect();
    for (file, line) in FOUND_BY_THE_REVIEWS {
        assert!(
            cited
                .iter()
                .any(|cite| cite.file == file && (cite.first..=cite.last).contains(&line)),
            "{file}:{line} has no override"
        );
    }
}

// The lead's ruling on the proof review's question: the override of the fix loop's round cap
// (fix-loop.md:21) follows the goal's principle for fix rounds (docs/goal.md, Principles per
// step) word for word. The test holds that one phrase only; the rest of the item is held by
// the rerun of the proof step superpowers-overrides-check, not by a test.
#[test]
fn the_fix_round_limit_follows_the_goal() {
    let text = pack("packs/skill/superpowers.md");
    let overrides = items(text, "## Overrides");
    let fix_rounds = overrides
        .iter()
        .find(|item| {
            citations(item).iter().any(|cite| {
                cite.file == "subagent-driven-development/references/fix-loop.md"
                    && (cite.first..=cite.last).contains(&21)
            })
        })
        .expect("an override of the fix loop's round cap");
    assert!(
        fix_rounds
            .to_lowercase()
            .contains("past the limit, the lead decides and records it"),
        "{fix_rounds}"
    );
    assert!(!text.to_lowercase().contains("no ruling by the lead"));
}

#[test]
fn coding_pack_covers_each_term() {
    let text = pack("packs/domain/coding.md").to_lowercase();
    for term in [
        "slice",
        "size",
        "a bug fix starts with a test that reproduces the bug, committed failing before the fix",
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

// A check command runs without `-e`: a failed `$(...)` inside the scan's range would leave the
// range empty and the scan clean, so the merge base is taken first and the scan needs it.
#[test]
fn the_secret_scan_needs_the_merge_base_first() {
    let text = pack("packs/domain/coding.md");
    assert!(text.contains(
        "`base=\"$(shipit measure merge-base)\" && gitleaks git --redact --no-banner --log-opts=\"--text $base..HEAD\" .`"
    ));
    assert!(!text.contains("--log-opts=\"$(shipit"));
}
