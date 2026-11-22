{
  description = "ShipIt: checks and receipts for agent work";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    { nixpkgs, crane, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f system nixpkgs.legacyPackages.${system});

      perSystem =
        system: pkgs:
        let
          fs = pkgs.lib.fileset;
          craneLib = crane.mkLib pkgs;

          # The Rust source: cargo files, `.rs` files, the lint and format settings, and
          # `data/` when a slice adds it (build.rs embeds it), and the test fixtures the
          # unit tests include.
          src = fs.toSource {
            root = ./.;
            fileset = fs.unions [
              (craneLib.fileset.commonCargoSources ./.)
              ./clippy.toml
              ./rustfmt.toml
              (fs.maybeMissing ./data)
              ./tests/fixtures
            ];
          };

          commonArgs = {
            inherit src;
            strictDeps = true;
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          releaseArgs = commonArgs // {
            inherit cargoArtifacts;
          };

          # Checks run in the dev profile (debug assertions, overflow checks, no LTO).
          devArgs = commonArgs // {
            CARGO_PROFILE = "dev";
          };
          devArtifacts = craneLib.buildDepsOnly devArgs;
          checkArgs = devArgs // {
            cargoArtifacts = devArtifacts;
          };

          # No wrapper: the binary uses `git` and `bash` from PATH.
          shipit = craneLib.buildPackage (
            releaseArgs
            // {
              doCheck = false;
              meta.mainProgram = "shipit";
            }
          );

          # Portable release binary: statically linked against musl, so it runs on any
          # x86_64 Linux. The default package links against /nix/store.
          muslCraneLib = crane.mkLib pkgs.pkgsStatic;
          muslArgs = commonArgs // {
            doCheck = false;
          };
          shipitStatic = muslCraneLib.buildPackage (
            muslArgs
            // {
              cargoArtifacts = muslCraneLib.buildDepsOnly muslArgs;
              meta.mainProgram = "shipit";
            }
          );

          onlyX86Linux = pkgs.lib.optionalAttrs (system == "x86_64-linux");
        in
        {
          packages = {
            default = shipit;
          }
          // onlyX86Linux { static = shipitStatic; };

          checks = {
            package = shipit;

            clippy = craneLib.cargoClippy (
              checkArgs
              // {
                cargoClippyExtraArgs = "--all-targets -- -D warnings";
              }
            );

            test = craneLib.cargoTest (
              checkArgs
              // {
                nativeBuildInputs = [ pkgs.git ];
              }
            );

            doc = craneLib.cargoDoc (
              checkArgs
              // {
                cargoDocExtraArgs = "--no-deps --document-private-items";
                RUSTDOCFLAGS = "-D warnings";
              }
            );

            # Every `.rs` file is found by rule: cargo fmt cannot see the files that
            # build.rs generates `mod` lines for.
            fmt =
              pkgs.runCommand "shipit-fmt"
                {
                  nativeBuildInputs = [
                    pkgs.rustfmt
                    pkgs.findutils
                  ];
                }
                ''
                  cd ${src}
                  files=$(find . -type f -name '*.rs')
                  if [ -z "$files" ]; then echo "no Rust files found" >&2; exit 1; fi
                  # shellcheck disable=SC2086
                  rustfmt --check $files
                  touch "$out"
                '';

            bats =
              pkgs.runCommand "shipit-bats"
                {
                  nativeBuildInputs = [
                    pkgs.bats
                    pkgs.git
                    pkgs.jq
                    pkgs.bash
                    pkgs.actionlint
                    pkgs.shellcheck
                    pkgs.pmd
                    pkgs.python3Packages.lizard
                  ];
                  tests = fs.toSource {
                    root = ./.;
                    fileset = fs.unions [
                      ./tests
                      ./src
                      ./CLAUDE.md
                      (fs.maybeMissing ./.claude-plugin)
                      (fs.maybeMissing ./hooks)
                      (fs.maybeMissing ./agents)
                      (fs.maybeMissing ./bin)
                      (fs.maybeMissing ./scripts)
                      (fs.maybeMissing ./harness)
                      (fs.maybeMissing ./skills)
                      ./docs
                      ./README.md
                    ];
                  };
                }
                ''
                  cp -r "$tests" source
                  chmod -R u+w source
                  cd source
                  patchShebangs .
                  SHIPIT_BIN=${shipit}/bin/shipit bats tests
                  touch "$out"
                '';

            lint =
              pkgs.runCommand "shipit-lint"
                {
                  nativeBuildInputs = [
                    pkgs.bash
                    pkgs.shellcheck
                    pkgs.actionlint
                    pkgs.findutils
                  ];
                  lintSrc = fs.toSource {
                    root = ./.;
                    fileset = fs.unions [
                      ./tests
                      ./.github
                      ./.shellcheckrc
                      (fs.maybeMissing ./data)
                      (fs.maybeMissing ./bin)
                      (fs.maybeMissing ./scripts)
                    ];
                  };
                }
                ''
                  cp -r "$lintSrc" source
                  chmod -R u+w source
                  cd source

                  # Files are found by rule, never by a list: every file whose first line
                  # is a bash or sh shebang, or `# shellcheck shell=bash` or `shell=sh`, is
                  # shellchecked.
                  bash_files=()
                  sh_files=()
                  while IFS= read -r -d "" file; do
                    IFS= read -r first < "$file" || true
                    case $first in
                      '#!'*'/bash' | '#!'*'/bash '* | '#!'*' bash' | '#!'*' bash '* | '# shellcheck shell=bash') bash_files+=("$file") ;;
                      '#!'*'/sh' | '#!'*'/sh '* | '#!'*' sh' | '#!'*' sh '* | '# shellcheck shell=sh') sh_files+=("$file") ;;
                    esac
                  done < <(find . -type f -print0)
                  if [ "''${#bash_files[@]}" -eq 0 ]; then echo "no bash files found" >&2; exit 1; fi
                  shellcheck "''${bash_files[@]}"
                  if [ "''${#sh_files[@]}" -gt 0 ]; then shellcheck "''${sh_files[@]}"; fi

                  find .github/workflows -type f \( -name '*.yml' -o -name '*.yaml' \) -print0 | xargs -0 -r actionlint

                  touch "$out"
                '';
          }
          // onlyX86Linux { static = shipitStatic; };

          devShells.default = pkgs.mkShell {
            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
            packages = [
              pkgs.rustc
              pkgs.cargo
              pkgs.clippy
              pkgs.rustfmt
              pkgs.rust-analyzer
              pkgs.bats
              pkgs.git
              pkgs.jq
              pkgs.shellcheck
              pkgs.actionlint
              pkgs.pmd
              pkgs.python3Packages.lizard
              pkgs.cargo-deny
              pkgs.gitleaks
            ];
          };
        };

      built = forAll perSystem;
      pick = name: nixpkgs.lib.mapAttrs (_system: attrs: attrs.${name}) built;
    in
    {
      packages = pick "packages";
      checks = pick "checks";
      devShells = pick "devShells";
    };
}
