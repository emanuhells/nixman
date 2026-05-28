{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  outputs = { nixpkgs, ... }: {
    nixosConfigurations.test = nixpkgs.lib.nixosSystem {
      modules = [ ./configuration.nix ];
    };
  };
}
