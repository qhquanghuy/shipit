# Claude model pack

Which Claude model does which job. `shipit init` copies `starting-setup.toml` into the setup file's
`[agents]` table; the setup file is what a project edits afterwards.

Tiers (a higher tier is a stronger, slower and dearer model):

1. haiku: reading and reporting.
2. sonnet: making.
3. opus: planning, testing and reviewing.
4. fable: the strongest; where it is not available, opus at high effort stands in.

Roles, with their tier and effort:

- scout: tier 1. It reads code and reports. After one retry it steps up to tier 2.
- planner: tier 3, high effort. Every later step reads the plan, so it is written one tier above the maker.
- maker: tier 2, high effort. It writes the code and the tests.
- tester: tier 3, medium effort. It drives the built tool as a user would, without reading the code.
- reader: tier 1. It explains the change without the plan; a reviewer compares that with the goal.

The rule behind the numbers: whoever reviews work is one tier above whoever made it, so a maker
at tier 2 is reviewed at tier 3. The effort cap (`high`) keeps every role at or below it; raising
it is the owner's decision.

`max_fix_rounds` is how many times a review's findings are fixed before the owner decides, and
`[agents.parallel]` caps the slices in flight (3) and the agents running at once (4).
