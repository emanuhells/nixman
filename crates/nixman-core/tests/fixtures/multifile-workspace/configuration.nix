{ config, pkgs, ... }:
{
  imports = [
    ./modules/base.nix
    ./modules/extras.nix
  ];
}
