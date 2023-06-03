{
  inputs = rec {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-23.05";
    utils = { url = "github:numtide/flake-utils"; };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.flake-utils.follows = "utils";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, utils, rust-overlay, ... }:
    utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        # Get a specific rust version
        rust = pkgs.rust-bin.stable.latest.default;

        rustPlatform = pkgs.makeRustPlatform {
          cargo = rust;
          rustc = rust;
        };

        macDeps = with pkgs; [ darwin.apple_sdk.frameworks.Security ];
        linuxDeps = with pkgs; [ fuse3 ];

        ifTestable = block:
          if (pkgs.stdenv.isLinux && pkgs.stdenv.isx86_64) then
            block
          else
            rec { };
      in rec {
        packages = rec {
          zerostash = rustPlatform.buildRustPackage ({
            meta = with pkgs.lib; {
              description = "Secure, speedy, distributed backups";
              homepage = "https://symmetree.dev";
              license = licenses.mit;
              platforms = platforms.all;
            };

            name = "zerostash";
            pname = "0s";
            src = pkgs.lib.sources.cleanSource ./.;
            # buildFeatures = pkgs.lib.optionals pkgs.stdenv.isLinux [ "fuse" ];

            cargoLock = { lockFile = ./Cargo.lock; };

            nativeBuildInputs = with pkgs;
              [ pkg-config ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin macDeps;
            buildInputs = with pkgs;
              [ libusb ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux linuxDeps;
          } // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
            SODIUM_LIB_DIR="${pkgs.pkgsStatic.libsodium}/lib";
          });

          vm = self.nixosConfigurations.test.config.system.build.vm;

          default = zerostash;
        } // (ifTestable rec {
          nixosTest = import ./nix/nixos-test.nix {
            inherit (self) nixosModule;
            inherit pkgs;
          };
        });

        apps = rec {
          zerostash = utils.lib.mkApp { drv = packages.zerostash; };

          vm = utils.lib.mkApp {
            drv = packages.vm;
            exePath = "/bin/run-nixos-vm";
          };

          default = zerostash;
        } // (ifTestable rec {
          nixosTest = utils.lib.mkApp {
            drv = packages.nixosTest.driver;
            exePath = "/bin/nixos-test-driver";
          };
        });

        devShell = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.default ];
          nativeBuildInputs = [ rust ];
        };

      }) // {
        nixosModule = { pkgs, ... }: {
          imports = [
            ./nix/zerostash-nixos-module.nix
            {
              nixpkgs.overlays = [
                (_: _: { zerostash = self.packages.${pkgs.system}.zerostash; })
              ];
            }
          ];
        };

        nixosConfigurations.test = nixpkgs.lib.nixosSystem {
          system = "x86_64-linux";
          modules =
            [ self.nixosModule (import ./nix/test-nixos-configuration.nix) ];
        };
      };
}
