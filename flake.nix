{
  description = "ShipIt: checks and receipts for agent work";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAll (pkgs: {
        default = pkgs.stdenvNoCC.mkDerivation {
          pname = "shipit";
          version = nixpkgs.lib.removeSuffix "\n" (builtins.readFile ./VERSION);
          src = self;
          nativeBuildInputs = [ pkgs.makeWrapper ];
          dontBuild = true;
          installPhase = ''
            runHook preInstall
            root=$out/share/shipit
            mkdir -p "$root" $out/bin
            # The whole source tree ships, so a later slice's folders need no edit here.
            cp -r . "$root"
            chmod -R u+w "$root"
            rm -rf "$root/tests" "$root/.github" "$root/notes" "$root/docs"
            patchShebangs "$root"
            makeWrapper "$root/bin/shipit" $out/bin/shipit \
              --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.bash pkgs.jq pkgs.git pkgs.coreutils pkgs.gnused ]}
            runHook postInstall
          '';
          meta.mainProgram = "shipit";
        };
      });

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = [ pkgs.bash pkgs.jq pkgs.git pkgs.bats pkgs.shellcheck pkgs.actionlint ];
        };
      });

      checks = forAll (pkgs: {
        default = pkgs.runCommand "shipit-checks"
          {
            src = self;
            nativeBuildInputs = [ pkgs.bash pkgs.jq pkgs.git pkgs.coreutils pkgs.findutils pkgs.bats pkgs.shellcheck pkgs.actionlint ];
          }
          ''
            cp -r "$src" source
            chmod -R u+w source
            cd source
            patchShebangs .

            # Files are found by rule, never by a list: every file whose first line
            # is a bash shebang (patchShebangs wrote its store path) or
            # `# shellcheck shell=bash` is shellchecked.
            bash_files=()
            while IFS= read -r -d "" file; do
              IFS= read -r first < "$file" || true
              case $first in
                '#!'*'/bash' | '#!'*'/bash '* | '#!'*' bash' | '#!'*' bash '* | '# shellcheck shell=bash') bash_files+=("$file") ;;
              esac
            done < <(find . -type f -print0)
            if [ "''${#bash_files[@]}" -eq 0 ]; then echo "no bash files found" >&2; exit 1; fi
            shellcheck "''${bash_files[@]}"

            bats tests

            find .github/workflows -type f \( -name '*.yml' -o -name '*.yaml' \) -print0 | xargs -0 -r actionlint

            touch "$out"
          '';
      });
    };
}
