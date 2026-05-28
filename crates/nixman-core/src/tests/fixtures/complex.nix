{ config, pkgs, ... }:
{
  networking = {
    hostname = "complex";
    firewall = {
      enable = true;
      allowedTCPPorts = [ 22 80 443 ];
    };
  };
  services.nginx = {
    enable = true;
    virtualHosts."example.com" = {
      root = "/var/www";
      enableACME = true;
    };
  };
  users.users.admin = {
    isNormalUser = true;
    extraGroups = [ "wheel" "docker" ];
  };
}
