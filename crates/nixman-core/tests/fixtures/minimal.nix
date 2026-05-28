{ config, pkgs, ... }:
{
  networking.hostname = "testbox";
  services.openssh.enable = true;
  environment.systemPackages = with pkgs; [ vim git ];
}
