use crate::pure::machine_notes::Notes;

pub(crate) const TOOLS: [&str; 15] = [
    "git",
    "bash",
    "gh",
    "jq",
    "rg",
    "fd",
    "nix",
    "cargo",
    "rustc",
    "node",
    "npm",
    "python3",
    "uv",
    "docker",
    "shellcheck",
];

const GIT_NEEDED: (u64, u64) = (2, 39);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToolState {
    Have(String),
    Missing,
    NoAnswer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Signal {
    Ci,
    Ssh,
    Display,
    Wayland,
}

// A person at a screen can answer a question; CI and a remote shell cannot, and on a system
// with a display server the absence of one means nobody is looking.
pub(crate) fn headless(os: &str, signals: &[Signal]) -> bool {
    if signals.contains(&Signal::Ci) || signals.contains(&Signal::Ssh) {
        return true;
    }
    let has_screen = signals.contains(&Signal::Display) || signals.contains(&Signal::Wayland);
    matches!(os, "linux" | "freebsd" | "openbsd" | "netbsd" | "dragonfly") && !has_screen
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Writability {
    Writable,
    ReadOnly,
    CanCreate,
    CannotCreate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SetupHealth {
    Ok,
    Missing,
    Invalid(String),
    NotARepository,
}

#[derive(Debug)]
pub(crate) struct Doctor {
    pub(crate) os: String,
    pub(crate) arch: String,
    pub(crate) headless: bool,
    pub(crate) agent_config: Writability,
    pub(crate) notes_file: bool,
    pub(crate) notes: Notes,
    pub(crate) tools: Vec<(String, ToolState)>,
    pub(crate) setup: SetupHealth,
}

// Below 2.39, or not `git version X.Y...` at all.
pub(crate) fn git_too_old(version: &str) -> bool {
    let number = |part: Option<&str>| {
        let digits: String = part?.chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<u64>().ok()
    };
    let mut parts = version
        .strip_prefix("git version ")
        .map(|rest| rest.split('.'));
    let major = number(parts.as_mut().and_then(Iterator::next));
    let minor = number(parts.as_mut().and_then(Iterator::next));
    match (major, minor) {
        (Some(major), Some(minor)) => (major, minor) < GIT_NEEDED,
        (Some(_), None) | (None, _) => true,
    }
}

// The first problem of a setup file's message: the second line of an `is invalid:` list, else
// the message itself.
pub(crate) fn first_problem(message: &str) -> String {
    let mut lines = message.lines();
    let first = lines.next().unwrap_or_default();
    if first.ends_with("is invalid:") {
        lines.next().unwrap_or(first).to_owned()
    } else {
        first.to_owned()
    }
}

fn agent_config_text(writability: Writability) -> &'static str {
    match writability {
        Writability::Writable => "writable",
        Writability::ReadOnly => "read-only",
        Writability::CanCreate => "missing (can be created)",
        Writability::CannotCreate => "missing (cannot be created)",
    }
}

fn tool_line(tool: &str, state: &ToolState) -> String {
    match state {
        ToolState::Have(version) if tool == "git" && git_too_old(version) => format!(
            "  {tool} {version} (too old: ShipIt needs {}.{} or newer)",
            GIT_NEEDED.0, GIT_NEEDED.1
        ),
        ToolState::Have(version) => format!("  {tool} {version}"),
        ToolState::Missing => format!("  {tool} missing"),
        ToolState::NoAnswer => format!("  {tool} no answer"),
    }
}

pub(crate) fn report(doctor: &Doctor) -> Vec<String> {
    let mut lines = vec![
        format!(
            "machine: {} {}, headless: {}",
            doctor.os,
            doctor.arch,
            if doctor.headless { "yes" } else { "no" }
        ),
        format!(
            "agent config: ~/.claude/CLAUDE.md {}",
            agent_config_text(doctor.agent_config)
        ),
        if doctor.notes_file {
            "machine notes: ~/.config/shipit/machine.md".to_owned()
        } else {
            "machine notes: no machine notes yet (built-in list and recipe)".to_owned()
        },
        "tools:".to_owned(),
    ];
    lines.extend(
        doctor
            .tools
            .iter()
            .map(|(tool, state)| tool_line(tool, state)),
    );
    lines.push(match &doctor.setup {
        SetupHealth::Ok => "setup: .config/shipit.toml ok".to_owned(),
        SetupHealth::Missing => "setup: .config/shipit.toml missing".to_owned(),
        SetupHealth::Invalid(problem) => format!("setup: .config/shipit.toml invalid: {problem}"),
        SetupHealth::NotARepository => "setup: not in a git repository".to_owned(),
    });
    lines.push(format!("missing tool: {}", doctor.notes.recipe));
    let never: Vec<String> = doctor
        .notes
        .never
        .0
        .iter()
        .map(|words| words.join(" "))
        .collect();
    lines.push(format!("never: {}", never.join(", ")));
    lines
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::pure::machine_notes::parse_notes;

    fn signals() -> impl Strategy<Value = Vec<Signal>> {
        proptest::sample::subsequence(
            vec![Signal::Ci, Signal::Ssh, Signal::Display, Signal::Wayland],
            0..=4,
        )
    }

    fn state() -> impl Strategy<Value = ToolState> {
        prop_oneof![
            "[a-z0-9 .]{1,12}".prop_map(ToolState::Have),
            Just(ToolState::Missing),
            Just(ToolState::NoAnswer),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn headless_rule(
            os in prop_oneof![
                Just("linux".to_owned()), Just("freebsd".to_owned()), Just("macos".to_owned()),
                Just("windows".to_owned()), "[a-z]{1,8}",
            ],
            signals in signals(),
        ) {
            let remote = signals.contains(&Signal::Ci) || signals.contains(&Signal::Ssh);
            let screen = signals.contains(&Signal::Display) || signals.contains(&Signal::Wayland);
            let unix_with_servers = matches!(
                os.as_str(), "linux" | "freebsd" | "openbsd" | "netbsd" | "dragonfly"
            );
            prop_assert_eq!(headless(&os, &signals), remote || (unix_with_servers && !screen));
        }

        #[test]
        fn report_lists_every_tool_once(states in proptest::collection::vec(state(), 15)) {
            let doctor = Doctor {
                os: "linux".to_owned(),
                arch: "x86_64".to_owned(),
                headless: false,
                agent_config: Writability::Writable,
                notes_file: false,
                notes: parse_notes(None),
                tools: TOOLS.iter().map(|tool| (*tool).to_owned()).zip(states).collect(),
                setup: SetupHealth::Ok,
            };
            let lines = report(&doctor);
            for tool in TOOLS {
                let own = lines
                    .iter()
                    .filter(|line| line.strip_prefix("  ").is_some_and(|rest| {
                        rest.split(' ').next() == Some(tool)
                    }))
                    .count();
                prop_assert_eq!(own, 1, "{}", tool);
            }
            prop_assert_eq!(lines.len(), 4 + TOOLS.len() + 3);
        }

        #[test]
        fn old_git_is_flagged(major in 0_u64..4, minor in 0_u64..60, patch in 0_u64..20) {
            let line = format!("git version {major}.{minor}.{patch}");
            prop_assert_eq!(git_too_old(&line), (major, minor) < (2, 39));
            let apple = format!("git version {major}.{minor}.{patch} (Apple Git-154)");
            prop_assert_eq!(git_too_old(&apple), (major, minor) < (2, 39));
            let flagged = tool_line("git", &ToolState::Have(line));
            prop_assert_eq!(flagged.contains("too old"), (major, minor) < (2, 39));
        }

        #[test]
        fn a_version_that_is_not_git_version_x_y_is_flagged(text in "[a-z0-9 .]{0,16}") {
            prop_assume!(!text.starts_with("git version "));
            prop_assert!(git_too_old(&text));
            prop_assert!(git_too_old("git version 2"));
        }

        #[test]
        fn the_first_problem_is_one_line(
            label in "[a-z.]{1,8}",
            problems in proptest::collection::vec("[a-z.]{1,8}: [a-z ]{1,8}", 1..4),
        ) {
            let message = format!("{label} is invalid:\n{}", problems.join("\n"));
            prop_assert_eq!(first_problem(&message), problems.first().cloned().unwrap_or_default());
            prop_assert_eq!(first_problem(&format!("{label} is not valid TOML: x\ny")), format!("{label} is not valid TOML: x"));
        }
    }
}
