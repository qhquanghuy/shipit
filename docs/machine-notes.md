# Machine notes

The machine notes tell an agent what this machine has and what it must never do to it. They are one
file, `~/.config/shipit/machine.md`, the same on every project, and they are not part of any
repository. Claude Code loads them through one line in `~/.claude/CLAUDE.md`: `@~/.config/shipit/machine.md`.
`shipit doctor` prints the full facts; the file only holds the short part that is always loaded.

```
# Machine notes (run shipit doctor for the full list)
Missing here: jq, rg.
Missing tool: use `nix shell nixpkgs#<tool> --command <command>` for one run; to keep a tool, add it to the project's dev shell.
Never: `pip install`, `npm install -g`, `cargo install`
```

Two lines are read by tools; every other line is free text for the reader.

- A line starting `Missing tool: ` is the recipe an agent follows when a tool is missing.
- A line starting `Never: ` lists commands that are never run. Each entry is the text between a
  pair of backticks, split into words: the first word is the program, and every other word must be
  among its arguments.

When there is no file, or the file has no such line, the built-in list and recipe apply. The built-in
recipe: use what the project provides, ask the owner when a tool is still missing, never install it
globally. The built-in list is the global installs of the common package managers.

`shipit doctor` shows the recipe and the list it is using, and says `no machine notes yet` when the file is missing.
