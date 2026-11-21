// What the machine notes say that a tool reads: the recipe for a missing tool and the commands
// never to run. The file is `docs/machine-notes.md`'s example; every other line is free text.

// Each entry is words: the first is the program, the others must be among its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Never(pub(crate) Vec<Vec<String>>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Notes {
    pub(crate) never: Never,
    pub(crate) recipe: String,
}

pub(crate) const BUILT_IN_NEVER: &[&str] = &[
    "pip install",
    "pip3 install",
    "pipx install",
    "uv tool install",
    "npm install -g",
    "npm i -g",
    "npm install --global",
    "pnpm add -g",
    "yarn global add",
    "cargo install",
    "go install",
    "gem install",
    "brew install",
    "apt install",
    "apt-get install",
    "dnf install",
    "yum install",
    "pacman -S",
    "nix profile install",
    "nix-env -i",
    "nix-env --install",
];

pub(crate) const BUILT_IN_RECIPE: &str = "Use what the project provides (for example its nix develop shell). If a tool is still missing, ask the owner; never install it globally.";

const RECIPE_PREFIX: &str = "Missing tool: ";
const NEVER_PREFIX: &str = "Never: ";

fn built_in_never() -> Never {
    Never(
        BUILT_IN_NEVER
            .iter()
            .map(|entry| entry.split_whitespace().map(str::to_owned).collect())
            .collect(),
    )
}

// The text between each pair of backticks; an unclosed last span still counts.
fn code_spans(line: &str) -> Vec<Vec<String>> {
    line.split('`')
        .skip(1)
        .step_by(2)
        .map(|span| {
            span.split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|words| !words.is_empty())
        .collect()
}

pub(crate) fn parse_notes(text: Option<&str>) -> Notes {
    let lines: Vec<&str> = text.map(|text| text.lines().collect()).unwrap_or_default();
    let recipe = lines
        .iter()
        .find_map(|line| line.strip_prefix(RECIPE_PREFIX))
        .map(str::trim)
        .filter(|recipe| !recipe.is_empty())
        .map_or_else(|| BUILT_IN_RECIPE.to_owned(), str::to_owned);
    let never = lines
        .iter()
        .find_map(|line| line.strip_prefix(NEVER_PREFIX))
        .map(code_spans)
        .filter(|entries| !entries.is_empty())
        .map_or_else(built_in_never, Never);
    Notes { never, recipe }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn written(recipe: &str, never: &[String]) -> String {
        let spans: Vec<String> = never.iter().map(|entry| format!("`{entry}`")).collect();
        format!(
            "# Machine notes\nMissing here: jq.\n{RECIPE_PREFIX}{recipe}\n{NEVER_PREFIX}{}, \u{2026}\n",
            spans.join(", ")
        )
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn notes_round_trip(
            recipe in "[a-z][a-z .]{0,20}[a-z]",
            never in proptest::collection::vec("[a-z-]{1,6}( [a-z-]{1,6}){0,2}", 1..4),
        ) {
            let notes = parse_notes(Some(&written(&recipe, &never)));
            prop_assert_eq!(notes.recipe, recipe);
            let words: Vec<Vec<String>> = never
                .iter()
                .map(|entry| entry.split(' ').map(str::to_owned).collect())
                .collect();
            prop_assert_eq!(notes.never, Never(words));
        }

        #[test]
        fn no_lines_means_built_in(text in "[a-z ,.`\n]{0,40}") {
            let built_in = parse_notes(None);
            prop_assert_eq!(&built_in.recipe, BUILT_IN_RECIPE);
            prop_assert_eq!(built_in.never.0.len(), BUILT_IN_NEVER.len());
            prop_assert_eq!(parse_notes(Some(&text)), built_in);
        }
    }

    #[test]
    fn an_empty_recipe_or_list_falls_back_to_the_built_in_one() {
        let notes = parse_notes(Some("Missing tool: \nNever: nothing in backticks\n"));
        assert_eq!(notes, parse_notes(None));
    }
}
