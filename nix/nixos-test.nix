{ nixosModule, pkgs }:
pkgs.nixosTest ({ ... }: {
  name = "zerostash";
  nodes = {
    zerostash = { ... }:
      {
        imports = [
          nixosModule
          ./test-nixos-configuration.nix
        ];
      };
  };
  testScript = ''
    start_all()
    zerostash.wait_for_unit("default.target")
    zerostash.succeed("ls /zerostash-destination/005d19bf*497a0be1")
  '';
})
