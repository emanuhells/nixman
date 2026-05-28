{ config, pkgs, ... }:
{
  imports = [ ./hardware-configuration.nix ];

  networking.hostname = "testbox";
  networking.firewall.enable = true;
  networking.firewall.allowedTCPPorts = [ 22 80 443 ];

  services.openssh.enable = true;
  services.nginx.enable = false;

  environment.systemPackages = with pkgs; [
    vim
    git
    htop
  ];

  system.stateVersion = "25.11";
}
