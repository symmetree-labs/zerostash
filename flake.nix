{
  inputs = rec {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-24.05";
    utils = { url = "github:numtide/flake-utils"; };
  };

  outputs = { self, nixpkgs, utils, ... }:
    utils.lib.eachDefaultSystem (system:
      let
        overlays = [ ];
        pkgs = import nixpkgs { inherit system overlays; };

        fuseEnabled = pkgs.stdenv.isLinux;
        linuxDeps = pkgs: with pkgs; [ fuse3 ];
        macDeps = pkgs:
          with pkgs; [
            macfuse-stubs
            darwin.apple_sdk.frameworks.Security
            darwin.apple_sdk.frameworks.SystemConfiguration
            darwin.apple_sdk.frameworks.CoreServices
          ];

        buildFlags = "-p zerostash -p zerostash-files"
          + pkgs.lib.optionalString fuseEnabled " -p zerostash-fuse";

        features = pkgs.lib.optionals fuseEnabled [ "fuse" ];

        ifTestable = block:
          if (pkgs.stdenv.isLinux && pkgs.stdenv.isx86_64) then
            block
          else
            rec { };

        zstashpkg = pkgs:
          pkgs.rustPlatform.buildRustPackage ({
            meta = with pkgs.lib; {
              description = "Secure, speedy, distributed backups";
              homepage = "https://symmetree.dev";
              license = licenses.mit;
              platforms = platforms.all;
            };

            name = "zerostash";
            pname = "0s";
            src = pkgs.lib.sources.cleanSourceWith {
              src = pkgs.lib.sources.cleanSource ./.;
              filter = name: type:
                let baseName = baseNameOf (toString name);
                in !(".github" == baseName
                  || ("nix" == baseName && type == "directory"));
            };

            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "infinitree-0.11.0" =
                  "sha256-0iZwmRYAr2NAMjmN9I2ysD/ayoIQh8mNq3lV9eVXFX4=";
              };
            };

            buildFeatures = features;
            cargoCheckFeatures = features;

            cargoBuildFlags = buildFlags;
            cargoTestFlags = buildFlags;

            nativeBuildInputs = with pkgs;
              [ stdenv.cc.cc.lib pkg-config ]
              ++ pkgs.lib.optionals pkgs.stdenv.isDarwin (macDeps pkgs);
            buildInputs = with pkgs;
              [ libusb ]
              ++ pkgs.lib.optionals pkgs.stdenv.isLinux (linuxDeps pkgs)
              ++ pkgs.lib.optionals pkgs.stdenv.isDarwin (macDeps pkgs);
          } // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
            RUSTFLAGS =
              "-L${pkgs.stdenv.cc.cc}/lib/gcc/${pkgs.stdenv.targetPlatform.config}/${pkgs.stdenv.cc.cc.version} -lc";
            SODIUM_LIB_DIR = "${pkgs.pkgsStatic.libsodium}/lib";
          });
      in rec {
        packages = rec {
          zerostash = zstashpkg pkgs;
          zerostash-static = zstashpkg pkgs.pkgsStatic;

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

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs;
            [
              iconv
              cargo-edit
              clippy
              rust-analyzer
              rustfmt
              cargo-workspaces
              rustc
              cargo
            ] ++ self.packages.${system}.default.buildInputs;
          RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;
        };

        formatter = pkgs.nixfmt;

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
