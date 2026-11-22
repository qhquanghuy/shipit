# ShipIt in Claude Code

ShipIt installs as a Claude Code plugin from its own repository. In a ShipIt project (a repository
whose `.config/shipit.toml` is in the working tree or at `HEAD`) its hooks enforce four gates. In
any other project every hook gives no decision: the plugin's own build and the clean-up of its
agent ledgers still run. The files that only Claude Code needs are listed in
`harness/claude-code.txt`.

The hooks are guardrails against forgetting, not security. The real ship gate is the owner's merge,
protected on GitHub: branch protection on the base branch, and rules for tags and releases.

## Install

```
claude plugin marketplace add qhquanghuy/shipit
claude plugin install shipit@shipit
```

A local folder is loaded with Claude Code's plugin folder option. The plugin needs Nix: at the start
of a session it builds ShipIt into its own folder (`target/plugin/shipit`). This breaks the advice
not to write state in a plugin folder on purpose: the tool must match the plugin's version, and an
update brings a new folder. The first session waits for the build. Until a build exists, Bash in a
ShipIt project is blocked with the command to run, and the review gate and the agent cap are off;
other projects are untouched. The tool is on the agents' `PATH` as `shipit`. claude.ai and Cowork do not install a plugin that has a `bin/` folder.

## The gates

- **Review gate.** A reviewer of the work (`shipit:work-reviewer`) or a tester (`shipit:tester`)
  starts only when `shipit verify` passes. Put a line `shipit-branch: <local slice branch>` in the
  prompt: the receipt is then judged for that branch, whatever the folder holds. Without the line,
  the working copy is judged, and a base branch is refused. A denial says to run `shipit check`.
  `shipit:plan-reviewer` and `shipit:reader` are not gated.
- **Ship gate.** Merging a pull request, pushing to the base branch, pushing a tag and creating a
  release each stop for the owner's answer, also in auto mode. In bypass mode the owner cannot be
  asked, so they are refused. `shipit complete <goal> --confirm` is an owner action too; the dry run
  `shipit complete <goal>` passes.
- **Install gate.** A global install is refused, and the agent gets the recipe of the machine notes
  (`docs/machine-notes.md`).
- **Agent cap.** At most `agents.parallel.agents` agents run at once in a session, read from the
  base branch's setup. A start in flight counts for 120 seconds; a `Stop` of the session drops what
  the session no longer lists as running.

## What the ship gate lets through

The gate judges the commands of Bash and Monitor, and the tools of MCP servers. It reads a command
as plain shell and asks for anything it cannot read, so write plain commands and name branches in
plain text. The only silent push is an explicit push of a slice branch, in a command that does not
also switch, merge, rebase, reset, pull, configure or change folder, in a repository whose config
does not send a push elsewhere:

```sh silent
git push -u origin slice/v1/demo
git push origin HEAD:refs/heads/slice/v1/demo
```

Every other push asks, and in bypass mode is refused, and so does every write with `gh`:

```sh asks
git push
git push origin main
git push origin v1.0.0
git push -u origin "$BRANCH"
git checkout main && git merge slice/v1/demo && git push
gh pr merge 1
gh release create v1.0.0
```

A heredoc commit message and read-only commands such as `git log @{u}..` pass. A message is text
only when the command starts with `git` or `gh`. Also asking: any `GIT_` or `GH_` variable, a
change of the git config or remotes, `send-pack`, a shell or interpreter given a string to run, a
comment, and an MCP tool that writes to a branch that is not a slice branch.

## Sessions

Start the lead with `claude -n "$(shipit session-name <goal>)"`, and a slice in its own working
copy with `claude -n "$(shipit session-name <goal> <slice>)"`. A session answers to its name, so a
slice session ends with one message to the lead. A lead that is not in bypass mode holds a message
from a session that is (and the other way round) until the owner approves it, and drops it after
five minutes. So the lead confirms with `shipit status`, which is the source of truth; a message is
only a nudge. Names with a middle dot must be quoted in a typed `@` mention.

## Limits

Known, and not caught:

- git aliases, `gh alias`, git's own helpers by path, scripts and task runners, `curl` to the API,
  `gh workflow run`;
- a program name built by a glob or a variable (`gi[t] push`, `$x push`), and any encoded script:
  a text gate cannot read what it does not spell out;
- false asks, on purpose: a word `eval`, `xargs`, `exec`, `su`, `ssh` or `watch` in a command that
  names git or gh, `echo git push`, `ls ~/.config/gh`, a `for` loop over `git add`, a command
  substitution in such a command, `nix develop -c git commit -m …` that holds `git push` in the
  message, and `git config` with a key and no flag that reads;
- agents started outside the `Agent` tool, and a reviewer continued after the work changed;
- resumed agents and teammate messages count towards the cap but are never refused, and Claude
  Code's own internal agents may count too; whether a `Stop` always lists the background tasks is
  settled by the live test, and without the list a lost `SubagentStop` stays counted until the
  session starts again or is resumed;
- a session that runs for more than a week loses its count when another session starts;
- a session started outside a ShipIt project that later works inside one: the gates judge the
  folder of each event, the build and the launcher judge the folder the session started in;
- a session run with the bare option skips the plugin's hooks.

The real ship gate is the owner's merge, protected on GitHub. For more, add your own ask rules for
the commands you care about.
