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
    utils.lib.eachDefaultSystem
      (system:
        let
          pkgs = import nixpkgs { inherit system; };

          # Get a specific rust version
          mozilla = pkgs.callPackage (mozillapkgs + "/package-set.nix") { };
          rust = (mozilla.rustChannelOf {
            date = "2022-09-22";
            channel = "stable";
            sha256 = "8len3i8oTwJSOJZMosGGXHBL5BVuGQnWOT2St5YAUFU=";
          }).rust;

          naersk-lib = naersk.lib."${system}".override {
            cargo = rust;
            rustc = rust;
          };
        in
        rec {
          defaultPackage = packages.zerostash;
          defaultApp = apps.zerostash;
          apps.default = apps.zerostash;

          packages.zerostash = naersk-lib.buildPackage {
            meta = with pkgs.lib; {
              description = "Secure, speedy, distributed backups";
              homepage = "https://symmetree.dev";
              license = licenses.mit;
              platforms = platforms.all;
            };

            pname = "0s";
            name = "zerostash";
            version = "0.5.0";

            src = pkgs.lib.sourceFilesBySuffices ./. [ ".toml" ".rs" ];
            root = ./.;
          };

          apps.zerostash = utils.lib.mkApp { drv = packages.zerostash; };
          devShell = pkgs.mkShell {
            inputsFrom = [ self.defaultPackage.${system} ];
            nativeBuildInputs = with pkgs; [
              rust
            ];
          };

          packages.vm = self.nixosConfigurations.test.config.system.build.vm;
          apps.vm = utils.lib.mkApp { drv = packages.vm; exePath = "/bin/run-nixos-vm"; };

          packages.nixosTest = import ./nix/nixos-test.nix { inherit (self) nixosModule; inherit pkgs; };
          apps.nixosTest = utils.lib.mkApp { drv = packages.nixosTest.driver; exePath = "/bin/nixos-test-driver"; };
        }) //
    {
      nixosModule = { pkgs, ... }: {
        imports = [
          ./nix/zerostash-nixos-module.nix
          { nixpkgs.overlays = [ (_: _: { zerostash = self.packages.${pkgs.system}.zerostash; }) ]; }
        ];
      };

      nixosConfigurations.test = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          self.nixosModule
          (import ./nix/test-nixos-configuration.nix)
        ];
      };
    };
}
