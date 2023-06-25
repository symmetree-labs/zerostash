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
	virtualisation.graphics = false;
      };
  };

  testScript = ''
    start_all()
    zerostash.wait_for_unit("default.target")
    print(zerostash.succeed("ls /zerostash-destination/005d19bf*497a0be1"))

    zerostash.succeed("screen -dm 0s mount --keyfile /key.toml --target /mount_target /zerostash-destination")
    machine.wait_until_succeeds("pgrep 0s")

    print(zerostash.succeed('test "$(cat /mount_target/zerostash-source/test-file)" = "test"'))
  '';
})
