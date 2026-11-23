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
update brings a new folder. The first session waits for the build. Until a build exists, Bash,
Monitor and MCP calls in a ShipIt project are blocked with the command to run, and the review
gate and the agent cap are off; other projects are untouched. The tool is on the agents' `PATH`
as `shipit`. claude.ai and Cowork do not install a plugin that has a `bin/` folder.

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
  The same gate holds the author rule (The author rule).
- **Install gate.** A global install is refused, and the agent gets the recipe of the machine notes
  (`docs/machine-notes.md`).
- **Agent cap.** At most `agents.parallel.agents` agents run at once in a session, read from the
  base branch's setup. A start in flight counts for 120 seconds; a `Stop` of the session drops what
  the session no longer lists as running.

## What the ship gate lets through

The gate judges the commands of Bash and Monitor, and the tools of MCP servers. It reads a command
as plain shell and asks for anything it cannot read, so write plain commands and name branches in
plain text. For git it is a list: in a command that names git, only these pass without a question.

- Global options: only `-C <dir>` and `no-pager`. Any other, such as `-c`, runs a program or
  changes the config.
- Subcommands that read: `status`, `log`, `diff`, `show`, `rev-parse`, `rev-list`, `ls-files`,
  `ls-remote`, `blame`, `grep`, `describe`, `shortlog`, `cat-file`, `merge-base`, `name-rev`,
  `for-each-ref`, `fetch`, `ls-tree`, `show-ref`, `cherry`, `range-diff`, `check-ignore`, `help`,
  `version`.
- Subcommands that edit the work and the branches: `add`, `rm`, `mv`, `restore`, `commit`,
  `switch`, `checkout`, `branch`, `stash`, `reset`, `cherry-pick`, `revert`, `worktree`, `clean` (as
  `reset` passes) and `init` (not with `--template` or `--separate-git-dir`, which copy hooks or
  move the repository); also `apply` only to check (`--check`, `--stat`, `--numstat`,
  `--summary`: a patch can add a symbolic link into `.git`); `rebase` without an option
  that runs a program, `merge` while the branch is not a base, `tag` to list, `notes` to read,
  `config` and `remote` to read, `reflog` without `expire` or `delete`, and `symbolic-ref` with
  one name and no `-d` or `-m`.
- Everything else asks, such as `submodule`, `bisect`, `filter-branch`, `send-pack`, an alias or
  a typo, and an option that hands git a program to run.

When the gate cannot read a command it asks, and the ask says why: `cannot be read as plain
commands by ShipIt: it holds <cause>; it may ship (write plain commands, and name branches in plain
text)`. The cause is one of these:

- a `$` or a backquote other than `$?`
- a subshell, a group or a shell keyword (`(`, `{`, `!`, `if`, `for`, `while`, `case`, `function` and the like)
- a `~` other than in `cd ~/<folder>`
- a comment (a word that starts with `#`)
- a brace expansion (`{a,b}` or `{1..3}`)
- a backslash outside quotes
- a quote or a heredoc that is not closed
- a heredoc ShipIt does not read (`<<<`, no delimiter, or an unquoted delimiter whose text has `$`, a backquote or a backslash)
- a redirect ShipIt does not read (`<(`, `>|`, `<>` or no target)
- a folder ShipIt does not follow (`cd` alone, with `-`, an option, a glob, `$?` or two folders, a `cd` that may not run, `pushd`, `popd`, or a git `-C`, `--git-dir` or `--work-tree` that holds `$?`)

`$?` is read as text, so `echo "exit=$?"` passes; a `cd`, or a git `-C`, `--git-dir` or `--work-tree`, whose folder holds it does not.

The only silent push is an explicit push of a slice branch, in a command that does not also
switch, merge, rebase, reset, configure or change folder, in a repository whose config does not
send a push elsewhere (a remote's push or mirror setting, `push.default` other than `simple` or
`current`, `help.autocorrect`):

```sh silent
git push -u origin slice/v1/demo
git push origin HEAD:refs/heads/slice/v1/demo
```

A command is judged in the working folder and in every folder it changes to with `cd`, so a
ship command cannot hide behind a `cd` to a folder that is no project. `cd ~/dir` is read, with
`~` as `$HOME`; any other tilde asks. A folder is in a ShipIt project when the repository is: the
setup file is in the working tree, at `HEAD`, at the tip of the default branch (origin's `HEAD`;
without it `origin/main`, `origin/master`, then `main` or `master`), or the store folder of the
repository exists. So a worktree or a checkout of a commit from before the setup file is judged
too; a repository whose only setup file is on another branch is no project. A session that starts
or resumes in a project makes its store folder (`<git common dir>/shipit/`), so that a commit, a
checkout or a branch change cannot make the repository look like no project; for the same reason
`git branch` with `-d`, `-D`, `-m`, `-M`, `-c`, `-C`, `-f` or a long form of them, and `git fetch`
with a refspec that holds a colon, ask. If the setup file is removed on purpose, a leftover
`.git/shipit/` keeps the repository gated: remove that folder to undo it. The reads and edits
that pass without a question:

```sh silent
cd ~/work/app && git status
git reflog -n 5
git ls-tree -r HEAD && git show-ref && git cherry -v main
git symbolic-ref --short HEAD && git push -u origin slice/v1/demo
git apply --check fix.patch && git clean -n
git help log && git version
git status; rg -n pattern src
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

The goals of the store, `<git common dir>/shipit/goals/`, are the one place under `.git` that
passes: the skills write them. Every `.git` in the command must be followed at once by
`/shipit/goals`, then the end of the word, a `/`, a blank, a quote or a shell separator, with no
`..` part after it and no `receipts/` anywhere. Anything else naming
`.git` asks, and so does `.git` as a path part of its own. A link made there reaches the hooks and
the receipts, and an archive can hold one, so in a text that names the goals only `mkdir`, `cp`
(with no option but `-p`, `-f`, `-n`, `-i`, `-v`, `-t`, `-T`, `--target-directory` and `--`), `cat`,
`ls`, `head`, `tail`, `wc`, `diff` and `shipit` may run, and nothing may be redirected into a file (`2>&1`
and `/dev/null` are fine). Any other program in the same command makes it ask, so write the store
copy as a command of its own.

```sh silent
mkdir -p .git/shipit/goals/v1 && cp a.md .git/shipit/goals/v1/roadmap.v1.md
cat .git/shipit/goals/v1/roadmap.v1.md
ls .git/shipit/goals
diff a.md .git/shipit/goals/v1/roadmap.v1.md
ls /path/to/repo/.git/shipit/goals/
shipit validate build-plan .git/shipit/goals/v1/slices/x/build-plan.md
```

```sh asks
cp a.md .git/shipit/goals/../../hooks/pre-push
ln -s ../../../hooks .git/shipit/goals/v1/h
cp -rL a .git/shipit/goals/v1/
cp -P link .git/shipit/goals/v1/h
git status; cp a.md .git/shipit/goals/v1/a.md
git branch -D main
git fetch . +old:refs/remotes/origin/main
cat a.md > .git/shipit/goals/v1/b.md
git apply fix.patch
git init --separate-git-dir ../elsewhere
BASH_ENV=x.sh git status
cd /other/repo && git push origin main
cp a.md .git/shipit/receipts/check/x.json
cat .git/shipit/goals/v1/a.md .git/config
ls .git/shipit/goalsx/
ls .git/shipit/goals~
cd .git && ls
git reflog expire --all
git symbolic-ref HEAD refs/heads/main
git status; sh script.sh
git status; source /dev/stdin
git status; rg --pre sh pattern
```

A heredoc commit message and commands such as `git add -A`, `git log @{u}..` and `git fetch` pass.
A message is text only when the command starts with `git` or `gh`, and the words of `echo`,
`grep`, `ls`, `cat` and a few like them are text. Also asking: any `GIT_` or `GH_` variable, a
path under `.git/` other than the goals, `.git` as a path part, a shell given a string or a file
to run (`sh -c …`, `bash f.sh`, `sh < f.sh`), `source` or `.` reading a pipe or a device such as
`/dev/stdin`, a command slot of a text program (`rg --pre`, `ag --pager`, `man -P`), any run of
`python`, `perl`, `ruby`, `node` or `script`, a comment, and an MCP tool that writes to a branch
that is not a slice branch. A line `shipit-branch: slice/v1/x.` may end in a full stop: a branch
name cannot.

## The author rule

A receipt names who made the work, and a lead agent cannot make the work by mistake. The rule has
two halves: hooks that stop a mistake inside Claude Code, and a check in the receipt.

**The hooks (inside Claude Code).** On a slice branch an agent that is not `shipit:maker` cannot
commit, cherry-pick, revert, merge, rebase, apply (`am`), pull or reset, cannot point a branch at
another commit (`checkout -B`, `switch -C`, `branch -f`, `update-ref`), and cannot edit a file git
would commit. The owner's own main session gets a question instead of a refusal: the rule
binds agents, not the owner's interactive session. In bypass mode and in a mode ShipIt does not
know the owner cannot be asked, so the question is a refusal. A maker commits with one plain
command that carries its trailer:

```sh
git commit -m "<message>" --trailer "ShipIt-Made-By: shipit:maker"
```

Every `ShipIt-Made-By` trailer of a command counts: a maker that adds another value, or a main
session that adds the maker's next to its own, is refused. The maker's trailer is the maker's alone
in every working copy of a ShipIt project, on any branch: a commit that carries it, made by anyone
else, is refused (answer A for an agent, G for the main session), so that it cannot be moved onto a
slice branch later.

When the author rule and the ship gate both answer one command (a commit joined to a push to the
base branch, say), the answer is the stronger of the two and carries both texts.

A folder counts when it is a working copy of a ShipIt project whose current branch is a slice
branch (`slice/<goal>/<slice>`). The hook that judges Bash and Monitor reads each git command with
its `cd` and its `-C`; a command that names git and one of these moves but is not all plain git
commands (`env`, `nix develop -c`, `sh -c`, `git -c`, a substitution) is judged as a commit with
no trailer. The same hook judges the file tools `Edit`, `Write` and `NotebookEdit`: a path that git
ignores and does not track (`notes/`, build output) is not the work and passes, and so is
everything under `.shipit/td/` and `.shipit/drive/`, by path, before its `.gitignore` exists; a tracked file, a new file and a file in a new folder are the work.

These are the answers, copied by the hooks:

- A, a move by an agent that is not the maker: `ShipIt: on a slice branch only shipit:maker commits or moves the work, so that a receipt names who made it. Start shipit:maker for this change.`
- B, a maker's commit without its trailer, or not one plain command: `ShipIt: a maker commits with one plain command that carries its trailer: git commit -m "<message>" --trailer "ShipIt-Made-By: shipit:maker".`
- C, an edit by an agent that is not the maker: `ShipIt: on a slice branch only shipit:maker edits the work (a file git would commit), so that a receipt names who made it. Start shipit:maker for this change.`
- F, the question to the owner's main session: `ShipIt: only shipit:maker changes a slice's work. Answer yes only if you, the owner, make this change yourself; commit it with --trailer "ShipIt-Made-By: owner".`
- G, the main session writing the maker's trailer: `ShipIt: only shipit:maker may write its own trailer; the main session is not the maker.`
- D, a start of `shipit:maker` outside a working copy on a slice branch (the Agent tool of this Claude Code has no `cwd` to start it elsewhere): `ShipIt: shipit:maker works in the slice's worktree, and this Claude Code cannot start an agent in another folder: run the slice in a session opened in its worktree (docs/claude-code.md, Sessions).`
- E, a start of `shipit:maker` at another rank than the setup's maker or its stand-in, with the model and the effort the setup's maker has (`sonnet` and `high` here): `ShipIt: start shipit:maker with model "sonnet" and effort "high", the setup's maker (agents.roles.maker), or its stand-in: the receipts name that rank as the author.`

The agent gate holds the start of the maker: `shipit:maker` starts only at the setup's maker rank
(its model, and its effort when the setup gives one) or the stand-in of its tier, with the `model` and
`effort` of the Agent call, and only in a working copy on a slice branch (the session's folder: a `cwd` in the call is not read, because this Claude Code ignores it). The rank is read from the base branch's setup, as the agent
cap is.

**The receipt.** For a slice started after this rule shipped, `shipit receipt` checks every commit
of the slice's own work: from the fork the check judged to the tip, no merges, and without the
commits of the slices it comes `after` in the roadmap (their branches `slice/<goal>/<slice>`,
local or on origin, when they exist). Each must carry the trailer `ShipIt-Made-By: shipit:maker`
or `ShipIt-Made-By: owner`. The author of a review of the work (`--author`) must be the setup's
maker (its tier, and its effort when the setup gives one) or its stand-in. A plan review keeps its
notice. The refusals exit 1 and write nothing:

- `shipit: --author <author> is not the setup's maker (<rank>): the work is the maker's, so its review names the setup's maker`
- `shipit: commit <commit> on <branch> has no "ShipIt-Made-By: shipit:maker" or "ShipIt-Made-By: owner" trailer: only shipit:maker, or the owner, makes the work of a slice (docs/claude-code.md)`

**The cut-over.** A slice is under the trailer rule only when its start entry in the logbook says
so: `shipit start` writes `"author_rule": true` from the release of this rule on. A slice that
was started before, or by hand under the stacking rule (it has no start entry), is told on stderr
and goes on: `shipit: <slice> was started before the author rule: the trailers of its commits are not checked`.
A rebase changes nothing: the mark is on the start, not on a commit. No commit gets a trailer
after the fact.

**Limits.** The trailer is a claim the receipt checks, not proof.
- A shell outside Claude Code can type the trailer, and a lead's Bash edit (`sed -i`, a redirect)
  is not seen: the hooks judge file tools and git commands, not what a program writes.
- A commit made off a slice branch and brought in by the maker is the maker's.
- The hooks check "a slice branch", not "the slice's linked worktree": a main working copy kept on
  a slice branch is judged as one.
- The owner's own change from a shell, or a suggestion applied on GitHub, needs the trailer
  `ShipIt-Made-By: owner` (amend the commit and add it);
  without it every later receipt of the work is refused until the owner adds it.
- A child slice whose parent was squash-merged and whose parent branch is gone sees the parent's
  commits as its own, with no trailer: after the parent merges, rebase the child onto the base
  (the stacking rule).
- A command the reader cannot parse (a substitution, a `cd` it does not follow) is judged in the
  session's folder only: from a main copy that is not on a slice branch, `cd <worktree> && git
  commit -m "$(cat m)"` is asked by the ship gate and not refused as a move of the work.
- An Edit through a symbolic link into a worktree is not judged: the path is compared with the
  working copy's top as written.
- A merge commit is not checked for a trailer, so content added while resolving a merge is not
  attributed.
- The maker runs at the setup's maker rank or its stand-in; a hard step at a higher rank needs a
  setup change by the owner, as a setup-only pull request.

## Sessions

Start the lead with `claude -n "$(shipit session-name <goal>)"`, and a slice in its own working
copy with `claude -n "$(shipit session-name <goal> <slice>)"`. The lead (or the owner) starts the slice in the main working copy, and `shipit start` prints the worktree's path; then the owner opens a new session in the worktree. The maker starts in the folder of the session, and works only on a slice branch. A session answers to its name, so a
slice session ends with one message to the lead. A lead that is not in bypass mode holds a message
from a session that is (and the other way round) until the owner approves it, and drops it after
five minutes. So the lead confirms with `shipit status`, which is the source of truth; a message is
only a nudge. Names with a middle dot must be quoted in a typed `@` mention.

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

**How slices run.** The last decision point of `shipit-roadmap` offers one session per slice, and the `Workflow` tool, which only the owner's explicit opt-in chooses.

The maker works only in a working copy on a slice branch, and the Agent tool of Claude Code 2.1.327 has no `cwd`: an agent it starts works in the folder of the session (measured). So no session in the main working copy can make a slice, and a headless run has one way too. The lead (or the owner) writes the build plan and starts the slice in the main working copy; `shipit start` prints the worktree's path; then the owner opens a new session in the worktree (`cd <path> && claude -n "$(shipit session-name <goal> <slice>)"`), which runs the rest of `shipit-slice`. `shipit:slice` writes and reviews a build plan, and ends.

**Nested agents.** Measured with the plugin loaded:
- The agent cap counts nested agents: an agent started by an agent holds a place, and `agents.parallel.agents` is to be sized for the widest moment of a session.
- The review gate fires for an agent started by an agent, so an agent that starts a gated role passes the `shipit-branch:` line as the lead does.
- By default Claude Code allows three layers of agents. Only `shipit:slice` has the `Agent` tool; setting `CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH` to 2 closes the third layer for every other agent too.
- A slice agent cannot ask the owner. A decision point ends it as blocked, its final message carries the decision point, and the lead asks the owner and starts a new slice agent.

**Pushes, pull requests and the store.** A skill pushes only `git push -u origin slice/<goal>/<slice>`, alone in its Bash call. It opens the pull request with `gh pr create --title <title> --body-file <file>`. `gh pr create`, `gh pr edit` and `gh pr comment` pass the gate in every mode, so no gate stops a gh write: the skills' rule, to post to a pull request only when the owner says yes, is prose. Store files are put in with `shipit store put`, because the harness protects the git folder: a file-tool write there, or a Bash command that names a path under it, asks in default mode, goes to the classifier in auto mode, and is refused in a headless run, and an allow rule does not change that; `shipit store put` names no such path, and ShipIt writes the file. A goal or roadmap version is the owner's decision and is still copied with `mkdir -p … && cp …`, which asks.

### Permissions the skills need

The skills run commands and write files. In a headless run (`claude -p`) every question becomes a refusal, so give the permissions first. Put this in `.claude/settings.json` at the root of the project and commit it alone as the first adoption pull request (Adopt ShipIt in a project); or give the same rules with `--allowedTools`. Replace `./test.sh` with your project's own test command. The skills read and search with the Read and Grep tools, so the list has no `cat`, `grep` or `ls` rule. An installed plugin is read from its cache folder, as the last rule shows; a plugin loaded from a folder of your own needs `Read(//<plugin folder>/**)` instead (`//` starts an absolute path).

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
      "Bash(git switch -c *)",
      "Bash(git checkout -b *)",
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

**Editing files and branches.** The list has no `Edit` rule. Add `--permission-mode acceptEdits` to each headless run (`claude -p --permission-mode acceptEdits ...`), which lets the session edit the files of the project, its worktree and `.shipit/td/`, and nothing outside them; in an interactive session the owner accepts the edits. The list holds `git switch -c` and `git checkout -b`, so that an agent can make an adoption branch (`adopt/setup`, `adopt/ci`, `adopt/settings`); `shipit start` makes the slice branches itself.

**Running the result.** The test drive runs the built program, and a maker runs the project's own check: give each a rule like the one for `./test.sh`, for example `Bash(./greet.sh *)` for a program `greet.sh`. They run from the worktree or from `.shipit/drive/<slice>`, both inside the project.

**The worktree.** `shipit start` prints `git worktree add <path> <branch>`, with the absolute path `.shipit/wt/<slice>` under the top of the main working copy, whatever folder or worktree it ran in: the worktree sits inside the project, where the session may read, write and run commands, so no `--add-dir` is needed (it needs a folder that exists, and the worktree is made later). `shipit start` adds `/.shipit/wt/` to `<git common dir>/info/exclude`, so git, and the search tools that read git's ignore rules, skip it in every working copy; nothing is committed. A tool of the project that walks the folder without git's rules (a test runner, a linter) may still see it: add `.shipit/wt` to its own ignore list.

**The store.** The harness asks for a write under the git folder and a headless run refuses it, whatever the allow rules say; `shipit store put` writes build plans, plan reviews and reviews instead, and `Bash(shipit *)` allows it. A goal or roadmap version is the owner's decision: its copy asks.

**Trust and headless runs.** Claude Code reads the project's `.claude/settings.json` only in a folder that was trusted, and a headless run never shows the trust question, so there the list above is not read. Trust the folder once in an interactive session (`claude` in the project folder) before the first headless run.

These commands are not in the list; they ask:

- Asks: the store copy (`mkdir -p` and `cp`): only for a goal or roadmap version, the owner's decision; the lead copies it, or hands the owner the exact commands.
- Asks: the review's link hash, once per link: its `printf`, piped to the hash.

## Limits

Known, and not caught:

- git aliases, `gh alias`, git's own helpers by path, scripts and task runners, `curl` to the API,
  `gh workflow run`;
- a symbolic link inside `.git/shipit/goals/` placed by other means (a file tool, a patch applied
  earlier, an archive): a text gate cannot see it, and only ShipIt and the skills write there;
- a write to `.git/hooks` with a file tool (no Bash call), and a path under `.git` named from a
  folder outside every project;
- a push from another repository whose remote is the project's own remote: that repository is
  judged by its own setup, not the project's;
- a program name built by a glob or a variable with no `git` or `gh` word in the text (`gi[t] push`,
  `$x push`), and any encoded script: a text gate cannot read what it does not spell out;
- an MCP write that names its branch in a field other than `branch`;
- false asks, on purpose: a word `eval`, `xargs`, `exec`, `su`, `ssh` or `watch` in a command that
  names git or gh, `ls ~/.config/gh`, a `for` loop over `git add`, a command substitution in such a
  command, `git clone`, `git pull`, `git submodule` and `git tag v1`, a `python` or `perl` run,
  `cd /other/repo && git push origin main` from inside a project (the working folder is always
  judged), a text that names `.git/`, `git apply`
  to apply a patch, `tar` or `mv` into the goals, `bash script.sh`
  and `bash -o x`, `git reflog expire`, `git init --template`, `nix develop -c git commit -m …`
  that holds `git push` in the message, and `git config` with a key and no flag that reads;
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
