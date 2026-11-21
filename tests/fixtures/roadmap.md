# Roadmap

Prose first. The settings below are not the roadmap.

```toml
[settings]
cap = 60
```

## Slices

The slices, in order. `beta` and `gamma` run in parallel after `alpha`.

```toml
[[slice]]
name = "alpha"
after = []
meets = ["DW1"]
owns = ["src/alpha.rs", "docs/alpha/"]
test_drive = false
branch = "slice/alpha"

[[slice]]
name = "beta"
after = ["alpha"]
meets = ["DW2"]
owns = ["src/beta.rs"]
test_drive = true
branch = "slice/beta"

[[slice]]
name = "gamma"
after = ["alpha"]
meets = ["DW3"]
owns = ["src/gamma.rs"]
test_drive = false
branch = "slice/gamma"

[[proof]]
name = "proof"
after = ["beta", "gamma"]
meets = []
how = "run by the owner"
```

## Notes

```toml
[[slice]]
name = "not-this-one"
```
