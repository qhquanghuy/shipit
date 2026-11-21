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
            cp -r bin libexec lib VERSION "$root"/
            patchShebangs "$root/bin"
            makeWrapper "$root/bin/shipit" $out/bin/shipit \
              --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.bash pkgs.jq pkgs.git pkgs.coreutils ]}
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

            # Files are found by rule, never by a list.
            find bin libexec -type f ! -name .keep -print0 | xargs -0 -r shellcheck
            find lib -type f -name '*.sh' -print0 | xargs -0 -r shellcheck
            find tests -type f -name '*.bash' -print0 | xargs -0 -r shellcheck

            bats tests

            find .github/workflows -type f \( -name '*.yml' -o -name '*.yaml' \) -print0 | xargs -0 -r actionlint

            touch "$out"
          '';
      });
    };
}
