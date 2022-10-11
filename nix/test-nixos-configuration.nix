{ lib, ... }:
{
  # Easy debugging via console and ssh
  # WARNING: root access with empty password
  networking.firewall.enable = false;
  services.getty.autologinUser = "root";
  services.openssh.enable = true;
  services.openssh.permitRootLogin = "yes";
  users.extraUsers.root.password = "";
  users.mutableUsers = false;

  # set up example stash
  system.activationScripts.setup-stash.text = ''
    set -e
    mkdir -p /zerostash-destination
    mkdir -p /zerostash-source
    if [[ ! -f /zerostash-source/test-file ]]; then
      echo "test" > /zerostash-source/test-file
    fi
  '';

  # zerostash service configuration
  services.zerostash = {
    enable = true;

    backups = {
      example = {
        paths = [ "/zerostash-source" ];
        options = [ "-x" ];
        timerConfig = lib.mkForce { };
        stash = {
          key = {
            source = "plaintext";
            user = "123";
            password = "123"; # DO NOT include your password in the NixOS configuration, use a keyfile!
          };
          backend = {
            type = "fs";
            path = "/zerostash-destination";
          };
        };
      };
    };
  };

  # run backup on startup
  systemd.services.zerostash-example.wantedBy = [ "multi-user.target" ];
}
