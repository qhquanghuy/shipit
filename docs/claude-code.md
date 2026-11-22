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
  `switch`, `checkout`, `branch`, `stash`, `reset`, `cherry-pick`, `revert`, `worktree`, `apply`
  (also with `--index`, as `add` passes; not with `--unsafe-paths`), `clean` (as `reset`
  passes) and `init` (not with `--template`, which copies hooks); also `rebase` without an option
  that runs a program, `merge` while the branch is not a base, `tag` to list, `notes` to read,
  `config` and `remote` to read, `reflog` without `expire` or `delete`, and `symbolic-ref` with
  one name and no `-d` or `-m`.
- Everything else asks, such as `submodule`, `bisect`, `filter-branch`, `send-pack`, an alias or
  a typo, and an option that hands git a program to run.

The only silent push is an explicit push of a slice branch, in a command that does not also
switch, merge, rebase, reset, configure or change folder, in a repository whose config does not
send a push elsewhere (a remote's push or mirror setting, `push.default` other than `simple` or
`current`, `help.autocorrect`):

```sh silent
git push -u origin slice/v1/demo
git push origin HEAD:refs/heads/slice/v1/demo
```

A command is judged by the folder git or gh runs in. After `cd` to a folder that is no ShipIt
project (`cd /other/repo && git push origin main`), and when no other folder of the command is
one, there is no decision. `cd ~/dir` is read, with `~` as `$HOME`; any other tilde asks. The
reads and edits that pass without a question:

```sh silent
cd ~/work/app && git push -u origin slice/v1/demo
git reflog -n 5
git ls-tree -r HEAD && git show-ref && git cherry -v main
git symbolic-ref --short HEAD && git push -u origin slice/v1/demo
git apply fix.patch && git clean -n
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
`/shipit/goals/`, with no `..` part after it and no `receipts/` anywhere. Anything else naming
`.git` asks, and so does `.git` as a path part of its own.

```sh silent
mkdir -p .git/shipit/goals/v1 && cp a.md .git/shipit/goals/v1/roadmap.v1.md
cat .git/shipit/goals/v1/roadmap.v1.md
ls /path/to/repo/.git/shipit/goals/
shipit validate build-plan .git/shipit/goals/v1/slices/x/build-plan.md
```

```sh asks
cp a.md .git/shipit/goals/../../hooks/pre-push
cp a.md .git/shipit/receipts/check/x.json
cat .git/shipit/goals/v1/a.md .git/config
ls .git/shipit/goalsx/
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
- a symbolic link inside `.git/shipit/goals/` that points elsewhere: a text gate cannot see it, and
  only ShipIt and the skills write there;
- a program name built by a glob or a variable with no `git` or `gh` word in the text (`gi[t] push`,
  `$x push`), and any encoded script: a text gate cannot read what it does not spell out;
- an MCP write that names its branch in a field other than `branch`;
- false asks, on purpose: a word `eval`, `xargs`, `exec`, `su`, `ssh` or `watch` in a command that
  names git or gh, `ls ~/.config/gh`, a `for` loop over `git add`, a command substitution in such a
  command, `git clone`, `git pull`, `git submodule` and `git tag v1`, a `python` or `perl` run,
  a text that names `.git/`, `.git/shipit/goals` without its closing slash, `bash script.sh`
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
