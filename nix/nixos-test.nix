{ nixosModule, pkgs }:
pkgs.nixosTest ({ ... }: {
  name = "zerostash";
  nodes = {
    zerostash = { ... }: {
      imports = [ nixosModule ./test-nixos-configuration.nix ];
      virtualisation.graphics = false;
    };
  };

  testScript = ''
    start_all()
    zerostash.wait_for_unit("default.target")
    print(zerostash.succeed("ls /zerostash-destination/2eea7df2ee11eed72e8597827645fe44c4f45857dbf90575b12eed09721fee74"))

    zerostash.succeed("screen -dm 0s mount --keyfile /key.toml --target /mount_target /zerostash-destination")

    machine.wait_until_succeeds("ls /mount_target/zerostash-source/test-file", timeout=10)
    print(zerostash.succeed('test "$(cat /mount_target/zerostash-source/test-file)" = "test"'))
  '';
})
