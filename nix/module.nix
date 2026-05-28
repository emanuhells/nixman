{ config, lib, pkgs, ... }:

let
  cfg = config.programs.nixman;
in
{
  options.programs.nixman = {
    enable = lib.mkEnableOption "nixman CLI, a terminal UI for NixOS configuration management";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./cli.nix {};
      defaultText = lib.literalExpression "pkgs.callPackage ./cli.nix {}";
      description = "The nixman package to install.";
    };
  };

  config = lib.mkIf cfg.enable {
    # Install the nixman package (defaults to CLI; set programs.nixman.package to override)
    environment.systemPackages = [ cfg.package ];

    # Ensure flakes are enabled (nixman requires them)
    nix.settings.experimental-features = lib.mkDefault [ "nix-command" "flakes" ];

    # Polkit rule: allow wheel users to run nixos-rebuild without password prompt
    security.polkit.extraConfig = ''
      polkit.addRule(function(action, subject) {
        if (action.id === "org.freedesktop.policykit.exec" &&
            action.lookup("program") === "/run/current-system/sw/bin/nixos-rebuild" &&
            subject.isInGroup("wheel")) {
          return polkit.Result.AUTH_ADMIN_KEEP;
        }
      });
    '';
  };
}
