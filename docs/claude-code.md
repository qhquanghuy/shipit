# ShipIt in Claude Code

ShipIt installs as a Claude Code plugin from its own repository. In a ShipIt project (a repository
whose `.config/shipit.toml` is in the working tree or at `HEAD`) its hooks enforce four gates. In
any other project every hook gives no decision: the plugin's own build and the clean-up of its
agent ledgers still run. The files that only Claude Code needs are listed in
`harness/claude-code.txt`.

The hooks are guardrails against forgetting, not security. They do not stop a merge, a push or a tag:
see Merging, pushing and tags.

## Install

```
claude plugin marketplace add qhquanghuy/shipit
claude plugin install shipit@shipit
```

A local folder is loaded with Claude Code's plugin folder option. The plugin needs Nix: at the start
of a session it builds ShipIt into its own folder (`target/plugin/shipit`). This breaks the advice
not to write state in a plugin folder on purpose: the tool must match the plugin's version, and an
update brings a new folder. The first session waits for the build. Until a build exists, Bash and
Monitor calls in a ShipIt project are blocked with the command to run, and the review
gate and the agent cap are off; other projects are untouched. The tool is on the agents' `PATH`
as `shipit`. claude.ai and Cowork do not install a plugin that has a `bin/` folder.

## The gates

- **Review gate.** A reviewer of the work (`shipit:work-reviewer`) or a tester (`shipit:tester`)
  starts only when `shipit verify` passes. Put a line `shipit-branch: <local slice branch>` in the
  prompt: the receipt is then judged for that branch, whatever the folder holds. Without the line,
  the working copy is judged, and a base branch is refused. A denial says to run `shipit check`.
  `shipit:plan-reviewer` and `shipit:reader` are not gated.
- **Install gate.** A global install is refused, and the agent gets the recipe of the machine notes
  (`docs/machine-notes.md`).
- **Agent cap.** At most `agents.parallel.agents` agents run at once in a session, read from the
  base branch's setup. A start in flight counts for 120 seconds; a `Stop` of the session drops what
  the session no longer lists as running.
- **Maker start.** `shipit:maker` starts only at the setup's maker rank (its model, and its effort
  when the setup gives one) or the stand-in of its tier, with the `model` and `effort` of the Agent
  call, and in one of two places. Started with `isolation: "worktree"`, it works in a new worktree of
  its own, and its prompt has a line `shipit-branch: <local slice branch>` (Parallel makers). Started
  without isolation, it works in the folder of the session (a `cwd` in the call is not read, because
  this Claude Code ignores it), which must be a working copy on a slice branch. The rank is read from
  the base branch's setup, as the agent cap is. The refusals:
  - A start without isolation outside a working copy on a slice branch (the Agent tool of this Claude Code has no `cwd` to start it elsewhere): `ShipIt: start shipit:maker with isolation "worktree" and a line "shipit-branch: <slice branch>" in its prompt, or from a session opened in the slice's worktree (docs/claude-code.md, Parallel makers).`
  - An isolated start whose prompt has no `shipit-branch:` line: `ShipIt: name the slice: add a line "shipit-branch: <branch>" to the prompt of shipit:maker.`
  - An isolated start whose line names a base branch or another revision, not a local slice branch: `ShipIt: "shipit-branch: <branch>" must name a local slice branch, not a base branch or another revision.`
  - A start at another rank than the setup's maker or its stand-in, with the model and the effort the setup's maker has (`sonnet` and `high` here): `ShipIt: start shipit:maker with model "sonnet" and effort "high", the setup's maker (agents.roles.maker), or its stand-in: the receipts name that rank as the author.`

  A review of the work names the setup's maker as `--author`, else `shipit receipt` refuses it:
  `shipit: --author <author> is not the setup's maker (<rank>): the work is the maker's, so its review names the setup's maker`

## Merging, pushing and tags

ShipIt stops none of these any more. Open to an agent now: `gh pr merge`, a push to the base
branch, a tag and its push, `gh release create`, `shipit complete <goal> --confirm`, and the merge
and push tools of MCP servers.

Branch protection with the required check proves that the check ran; it does not prove that the
owner decided. An agent that runs on the owner's gh login can still run `gh pr merge`.

What stops an agent, and only what is named here:

- a required approving review on the base branch stops a merge into the base branch (the agent
  cannot approve its own pull request), and only while admin bypass is off: an agent on the owner's
  admin gh login can still run `gh pr merge --admin`. It does not stop a tag, `gh release create`
  or `shipit complete <goal> --confirm`;
- a token for agents without merge, release or tag rights stops the actions that token lacks, and
  only for commands that run on that token;
- deny rules in the project's `.claude/settings.json` stop the commands they name, in the Claude
  Code session that reads them.

The optional deny rules (replace `main` with the base branch):

```json
{ "permissions": { "deny": ["Bash(gh pr merge *)", "Bash(git push origin main)", "Bash(git push origin main *)", "Bash(git tag *)", "Bash(gh release create *)", "Bash(shipit complete *)"] } }
```

A deny rule in the project settings also stops the owner's own session; the owner then runs these
commands in a plain terminal. `Bash(git tag *)` also blocks `git tag -l`. `Bash(shipit complete *)`
also blocks the owner's own dry run, so the owner may prefer to rely on their word for that one.
A prefix rule misses other forms (`git push origin HEAD:main`, an MCP tool), so it is a guardrail,
not a guard.

## Sessions

Start the lead with `claude -n "$(shipit session-name <goal>)"`, and a slice in its own working
copy with `claude -n "$(shipit session-name <goal> <slice>)"`. A slice that the owner runs as one session per slice has its own working copy: the lead (or the owner) starts the slice in the main working copy, `shipit start` prints the worktree's path, and the owner opens a new session in it. The other ways keep the slice in the lead's session, and each maker works in a worktree of its own (Parallel makers). A session answers to its name, so a
slice session ends with one message to the lead. A lead that is not in bypass mode holds a message
from a session that is (and the other way round) until the owner approves it, and drops it after
five minutes. So the lead confirms with `shipit status`, which is the source of truth; a message is
only a nudge. Names with a middle dot must be quoted in a typed `@` mention.

## Parallel makers

One lead session runs the makers of several slices at once. Each maker works in a worktree of its own, which Claude Code makes, and a git hook checks the commits made in that worktree against its slice branch.

1. The lead runs `shipit start` for each slice and makes no slice worktree: git refuses a branch that two worktrees hold.
2. The lead starts each maker with `isolation: "worktree"`, `run_in_background: true` and a first prompt line `shipit-branch: slice/<goal>/<slice>`. Several makers may start in one message.
3. The maker runs `git switch slice/<goal>/<slice>`, then `shipit pre-commit`. If git refuses, the maker reports and stops. If `shipit pre-commit` prints a line that starts `shipit: not checked`, the maker waits a few seconds, runs it once more (a launch is recorded a moment after the agent is up), and reports and stops if the line comes again.
4. The maker's last step runs `shipit check` in its worktree.
5. After a maker ends, the lead finds its worktree with `git worktree list`, then runs `git worktree remove .claude/worktrees/agent-<id>` and `git branch -d worktree-agent-<id>` from the main working copy, each alone in its call, before the next maker of that slice starts.

Never resume an isolated maker: start a new one. A resumed agent works in the session's folder, outside its worktree, where the hook cannot see it.

**The record.** The agent hooks write one append-only file, `<git common dir>/shipit-makers.jsonl`, outside the store folder. Every line is one JSON object, written whole, so parallel writers never mix:

| Line | Written by | When |
|---|---|---|
| `{"event":"start","session":…,"call":…,"branch":…}` | `shipit claude-agent-gate` | before the Agent tool starts an isolated `shipit:maker` whose prompt names a local slice branch |
| `{"event":"launched","session":…,"call":…,"agent":…}` | `shipit claude-agent-count` | after the same call, with the new agent's id |
| `{"event":"maker","session":…,"agent":…}` | `shipit claude-agent-count` | when a `shipit:maker` agent starts in its own worktree |
| `{"event":"failed","session":…,"call":…}` | `shipit claude-agent-count` | when the Agent call of an isolated `shipit:maker` fails, so its `start` line claims nothing |

The call id pairs a `launched` line with its `start` line, in any order of the lines.

**The commit hook.** `shipit init` adds a pre-commit hook next to the pre-push hook, with the same guards: `shipit pre-commit` runs only where ShipIt is installed. In a worktree Claude Code made for an agent (`<project>/.claude/worktrees/agent-<id>`), it reads the record. It refuses only a commit of a known maker on a branch that is not its slice's, and every other commit passes. A detached HEAD (a rebase stop, an amend) is a warning, not a refusal. The refusal, after `shipit: `:

- `this worktree is shipit:maker's for <branch>; commit there: git switch <branch> (docs/claude-code.md, Parallel makers)`

Where the commit cannot be judged, it passes with a warning on stderr:

- `shipit: not checked: this maker's slice branch is not known; start it again with run_in_background (docs/claude-code.md, Parallel makers)`
  A maker started in the foreground: its launch is recorded only when it ends.
- `shipit: not checked: no maker record names this worktree, but a maker was started for <branch> (docs/claude-code.md, Parallel makers)`
  A worktree that no record names, on a branch that an unclaimed start names: the `maker` line was lost.
- `shipit: not checked: HEAD is detached in this maker's worktree, so its commit cannot be matched to its slice branch <branch> (docs/claude-code.md, Parallel makers)`
  A known maker on a detached HEAD.

When `shipit pre-commit` runs in an agent worktree whose pre-commit hook file (the one git resolves, `core.hooksPath` included) is missing or has no `shipit pre-commit` line, it warns too, so the maker's own check does not pass in silence where the hook is off:

- `shipit: not checked: the pre-commit hook of this worktree has no shipit pre-commit line, so a commit here may not be checked (docs/claude-code.md, Parallel makers)`

When `shipit claude-agent-count` sees a launched maker start again outside its own worktree, it tells the agent as SubagentStart context: `ShipIt: not checked: agent <id> is an isolated shipit:maker started again outside its worktree; stop and tell the lead to start a new maker (docs/claude-code.md, Parallel makers)`.

**Limits.**
- A guardrail, not security: an agent can skip the hook with git's no-verify option, or edit the record file. The reviews and the receipt still judge the commits.
- A foreground maker's slice is not known until it ends, so its commits pass with the first warning above. The skills start every isolated maker in the background.
- The record is never pruned.
- The hooks that `shipit init` adds are off in an agent worktree when they are not there. A `lefthook-local.yml` lives only in the main working copy (it is per person and untracked). A new hooks file that is not committed is missing from the worktree, which Claude Code makes from the commits; `shipit init` says so. husky's `.husky/_` is generated by `husky install`, so a worktree has none. The maker's own `shipit pre-commit` warns in these cases.
- The hook sees only a commit made in the agent worktree itself. `git -C <main working copy>`, a `cd` to the main working copy, `git worktree move` and `-c core.hooksPath=…` all go round it.
- A hook that `shipit init` cannot add safely (a lefthook setup it does not write, a hook with `exit` or `exec`, a hooks folder outside the project) is printed with its reason, not written; until the owner adds it, the commit check is off there.
- The worktree path `.claude/worktrees/agent-<id>` is Claude Code's behaviour, measured on 2.1.327, not a contract.


## Skills

The plugin has five skills. The owner types them as `/shipit:shipit`, `/shipit:shipit-roadmap`, `/shipit:shipit-slice`, `/shipit:shipit-test-drive` and `/shipit:shipit-review` (the bare name works unless another command has it). Agents load them by themselves through the `Skill` tool. No skill sets `disable-model-invocation`, because an agent starts each of them at some point. The owner's view is `docs/guide.md`.

**Roles.** Every agent a skill starts is a named role of the plugin, so the screen shows `shipit:maker` and not a general agent. The `tools` line of each role file is checked by `tests/claude-plugin.bats`:

| Role | `tools` |
|---|---|
| `shipit:maker`, `shipit:tester` | `Read, Grep, Glob, Bash, Write, Edit, Skill` |
| `shipit:scout`, `shipit:reader`, `shipit:plan-reviewer`, `shipit:work-reviewer` | `Read, Grep, Glob, Bash` |
| `shipit:model-scout` | `Read, Grep, Glob, Bash, WebSearch, WebFetch` |
| `shipit:slice` | all tools |

Only `shipit:slice` may start agents: leaving `Agent` out of `tools` is what stops the others. Every start passes the `model` and `effort` of its rank; role files hold neither.

**Known gaps.**
- A review of a change that is not a slice, in a ShipIt project, starts two plain agents given the text of the `shipit:work-reviewer` role file, and they show as general agents. The review gate denies `shipit:work-reviewer` there, because the change has no slice branch for the `shipit-branch:` line to name.
- Outside a ShipIt session (no plugin loaded) the lead's own agents show as general ones.

**Sessions and messages.** Name a session with `claude -n "$(shipit session-name <goal> <slice>)"` as under Sessions; a slice session ends with one message to the lead, which confirms with `shipit status`.

**How slices run.** The third decision point of `shipit-roadmap` offers four ways, recommends one, and the owner picks:
- **Same session:** the lead runs `shipit-slice` itself, one slice at a time; each maker is started with isolation "worktree".
- **Subagents:** the lead runs the slices of a wave together; each maker is started with isolation "worktree". The lead recommends it for two or more slices in a wave.
- **One session per slice:** the lead (or the owner) writes the build plan and starts the slice in the main working copy; `shipit start` prints the worktree's path; then the owner opens a new session in the worktree (`cd <path> && claude -n "$(shipit session-name <goal> <slice>)"`), which runs the rest of `shipit-slice`. For a slice the owner wants to watch.
- **Claude Code workflow:** a workflow script the lead writes, each maker started with isolation "worktree". Not yet tried with ShipIt, so never recommended.

The Agent tool of Claude Code 2.1.327 has no `cwd`: an agent it starts works in the folder of the session (measured), unless it is started with isolation. The maker works only in a working copy on a slice branch, or in an isolated worktree on its slice branch, so the owner-run session in the main working copy is never where a maker works. `shipit:slice` writes and reviews a build plan, and ends.

**Nested agents.** Measured with the plugin loaded:
- The agent cap counts nested agents: an agent started by an agent holds a place, and `agents.parallel.agents` is to be sized for the widest moment of a session.
- The review gate fires for an agent started by an agent, so an agent that starts a gated role passes the `shipit-branch:` line as the lead does.
- By default Claude Code allows three layers of agents. Only `shipit:slice` has the `Agent` tool; setting `CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH` to 2 closes the third layer for every other agent too.
- A slice agent cannot ask the owner. A decision point ends it as blocked, its final message carries the decision point, and the lead asks the owner and starts a new slice agent.

**Pushes, pull requests and the store.** A skill pushes only `git push -u origin slice/<goal>/<slice>`, alone in its Bash call. It opens the pull request with `gh pr create --title <title> --body-file <file>`. No hook stops a gh write: the skills' rule, to post to a pull request only when the owner says yes, is prose. Store files are put in with `shipit store put`, because the harness protects the git folder: a file-tool write there, or a Bash command that names a path under it, asks in default mode, goes to the classifier in auto mode, and is refused in a headless run, and an allow rule does not change that; `shipit store put` names no such path, and ShipIt writes the file. A goal or roadmap version is stored the same way, with `shipit store put goal|roadmap`, once the owner approves it.

### Permissions the skills need

The skills run commands and write files. In a headless run (`claude -p`) every question becomes a refusal, so give the permissions first. Put this in `.claude/settings.json` at the root of the project and commit it alone as the first adoption pull request (Adopt ShipIt in a project); or give the same rules with `--allowedTools`. An agent cannot write that file (Claude Code protects it, also with `--permission-mode acceptEdits`): the agent prints this content, the owner creates `.claude/settings.json` with it, and then the agent commits it. Replace `./test.sh` with your project's own test command. The skills read and search with the Read and Grep tools, so the list has no `cat`, `grep` or `ls` rule. An installed plugin is read from its cache folder, as the last rule shows; a plugin loaded from a folder of your own needs `Read(//<plugin folder>/**)` instead (`//` starts an absolute path).

```json
{
  "permissions": {
    "allow": [
      "Bash(shipit *)",
      "Bash(git status *)",
      "Bash(git log *)",
      "Bash(git diff *)",
      "Bash(git show *)",
      "Bash(git rev-parse *)",
      "Bash(git fetch *)",
      "Bash(git grep *)",
      "Bash(git worktree add *)",
      "Bash(git worktree list *)",
      "Bash(git worktree remove .claude/worktrees/*)",
      "Bash(git branch -d worktree-agent-*)",
      "Bash(git switch -c *)",
      "Bash(git checkout -b *)",
      "Bash(git switch slice/*)",
      "Bash(git checkout slice/*)",
      "Bash(git add *)",
      "Bash(git commit *)",
      "Bash(git push -u origin slice/*)",
      "Bash(gh pr create *)",
      "Bash(gh pr view *)",
      "Bash(gh pr diff *)",
      "Bash(mkdir -p .shipit/td/*)",
      "Bash(mkdir -p .shipit/drive/*)",
      "Bash(sha256sum *)",
      "Bash(./test.sh *)",
      "Read(~/.claude/plugins/cache/shipit/**)"
    ]
  }
}
```

A skill makes its drafts folder inside the project, `.shipit/td/<name>` (`<name>` is the slice, or the goal for the roadmap), with `mkdir -p .shipit/td/<name>`, and writes the one line `*` to the `.gitignore` in it, so that git ignores the folder. The test drive has its own folder, `.shipit/drive/<slice>`, made the same way with `mkdir -p .shipit/drive/<slice>`, so that the tester sees only the built result, its docs and sample data, not the drafts. A folder inside the project is one the session may read, write and run commands in, so it needs no `--add-dir`.

**Editing files and branches.** The list has no `Edit` rule. Add `--permission-mode acceptEdits` to each headless run (`claude -p --permission-mode acceptEdits ...`), which lets the session edit the files of the project, its worktree and `.shipit/td/`, and nothing outside them; in an interactive session the owner accepts the edits. The list holds `git switch -c` and `git checkout -b`, so that an agent can make an adoption branch (`adopt/setup`, `adopt/ci`, `adopt/settings`); `shipit start` makes the slice branches itself. `git switch slice/*` and `git checkout slice/*` are for an isolated maker: it switches its new worktree to its slice branch, and carries the files of a slice that was over the cap onto a new part. `git worktree list`, `git worktree remove .claude/worktrees/*` and `git branch -d worktree-agent-*` let the lead find and free the worktree of a finished maker.

**Running the result.** The test drive runs the built program, and a maker runs the project's own check: give each a rule like the one for `./test.sh`, for example `Bash(./greet.sh *)` for a program `greet.sh`. They run from the worktree or from `.shipit/drive/<slice>`, both inside the project.

**The worktree.** `shipit start` prints `git worktree add <path> <branch>`, with the absolute path `.shipit/wt/<slice>` under the top of the main working copy, whatever folder or worktree it ran in: the worktree sits inside the project, where the session may read, write and run commands, so no `--add-dir` is needed (it needs a folder that exists, and the worktree is made later). `shipit start` adds `/.shipit/wt/` to `<git common dir>/info/exclude`, so git, and the search tools that read git's ignore rules, skip it in every working copy; nothing is committed. A tool of the project that walks the folder without git's rules (a test runner, a linter) may still see it: add `.shipit/wt` to its own ignore list. An isolated maker needs no such worktree, and none may hold its slice branch (Parallel makers in docs/claude-code.md).

**The store.** The harness asks for a write under the git folder and a headless run refuses it, whatever the allow rules say; `shipit store put` writes build plans, plan reviews and reviews instead, and `Bash(shipit *)` allows it. A goal or roadmap version is the owner's decision: once the owner approves it, the owner or the lead stores it with `shipit store put goal` or `shipit store put roadmap`, and every put is recorded (docs/store.md).

**Trust and headless runs.** Claude Code reads the project's `.claude/settings.json` only in a folder that was trusted, and a headless run never shows the trust question, so there the list above is not read. Trust the folder once in an interactive session (`claude` in the project folder) before the first headless run.

These commands are not in the list; they ask:

- Asks: the review's link hash, once per link: its `printf`, piped to the hash.

## Limits

Known, and not caught:

- agents started outside the `Agent` tool, and a reviewer continued after the work changed;
- resumed agents and teammate messages count towards the cap but are never refused, and Claude
  Code's own internal agents may count too; whether a `Stop` always lists the background tasks is
  settled by the live test, and without the list a lost `SubagentStop` stays counted until the
  session starts again or is resumed;
- a session that runs for more than a week loses its count when another session starts;
- a session started outside a ShipIt project that later works inside one: the gates judge the
  folder of each event, the build and the launcher judge the folder the session started in;
- a session run with the bare option skips the plugin's hooks.

Refused on purpose: an unreadable command that names an installer is refused by its words (for example `echo $HOME && echo pip install`); a known limit of DW4.

For merges, pushes and tags, see Merging, pushing and tags.
