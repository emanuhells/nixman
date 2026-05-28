{ config, pkgs, ... }:
{
  # Network configuration
  networking.hostname = "commented"; # Machine hostname

  # Enable SSH for remote access
  services.openssh = {
    enable = true; # TODO: restrict to specific IPs
    settings.PasswordAuthentication = false;
  };

  /* System packages */
  environment.systemPackages = with pkgs; [
    vim
    git
    htop
  ];
}
