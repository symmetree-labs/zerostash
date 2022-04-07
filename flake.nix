{
  inputs = {
    nixpkgs.url = "nixpkgs";
    utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, utils, naersk, ... }:
    utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        naersk-lib = naersk.lib."${system}";
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
          version = "0.3.0";

          src = ./.;
          root = ./.;

          LIBCLANG_PATH = "${pkgs.llvmPackages_13.libclang.lib}/lib";

          nativeBuildInputs = with pkgs; [
            llvm
            clang
          ];
          buildInputs = with pkgs; [
            llvm.dev
          ];
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

          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
        };
      });
}
