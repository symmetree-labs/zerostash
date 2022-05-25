{
  inputs = {
    nixpkgs.url = "nixpkgs";
    utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
    mozillapkgs.url = "github:mozilla/nixpkgs-mozilla";
    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, utils, naersk, mozillapkgs, ... }:
    utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        # Get a specific rust version
        mozilla = pkgs.callPackage (mozillapkgs + "/package-set.nix") { };
        rust = (mozilla.rustChannelOf {
          date = "2022-05-19";
          channel = "stable";
          sha256 = "oro0HsosbLRAuZx68xd0zfgPl6efNj2AQruKRq3KA2g=";
        }).rust;

        naersk-lib = naersk.lib."${system}".override {
          cargo = rust;
          rustc = rust;
        };
      in
      rec {
        packages.zerostash = naersk-lib.buildPackage {
          meta = with pkgs.lib; {
            description = "Secure, speedy, distributed backups";
            homepage = "https://symmetree.dev";
            license = licenses.mit;
            platforms = platforms.all;
          };

          name = "zerostash";
          version = "0.4.0";

          src = ./.;
          root = ./.;
        };
        defaultPackage = packages.zerostash;

        defaultApp = apps.zerostash;
        apps.zerostash = {
          type = "app";
          program = "${self.defaultPackage."${system}"}/bin/0s";
        };

        # `nix develop`
        devShell = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.zerostash ];
          nativeBuildInputs = with pkgs; [
            cargo
            rustc
            rust-analyzer
            rustfmt
            clippy
          ];
        };
      });
}
