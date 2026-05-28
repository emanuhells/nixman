{ config, lib, pkgs, ... }:
{
  boot.loader.grub.enable = true;
  fileSystems."/" = { device = "/dev/sda1"; fsType = "ext4"; };
}
