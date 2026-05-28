{
  description = "nixman — NixOS configuration management CLI";

  inputs = {
    nixpkgs.url     = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    let
      nixosModule = import ./nix/module.nix;
    in
    {
      nixosModules.default = nixosModule;
      nixosModules.nixman = nixosModule;

    } //
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in {

        packages.default = pkgs.callPackage ./nix/cli.nix {};
        packages.cli = pkgs.callPackage ./nix/cli.nix {};
        packages.mcp = pkgs.callPackage ./nix/mcp.nix {};

        apps.default = {
          type    = "app";
          program = "${self.packages.${system}.default}/bin/nixman";
        };

        apps.mcp = {
          type    = "app";
          program = "${self.packages.${system}.mcp}/bin/nixman-mcp";
        };

        devShells.default = pkgs.mkShell {
          name = "nixman-dev";

          nativeBuildInputs = with pkgs; [
            rustup
            pkg-config
          ];

          buildInputs = with pkgs; [
            openssl
            openssl.dev
            libgit2
          ];

          shellHook = ''
            rustup show active-toolchain > /dev/null 2>&1 || rustup show
            echo ""
            echo "  nixman dev shell"
            echo "  rustc: $(rustc --version 2>/dev/null || echo 'run: rustup show')"
            echo "  cargo build -p nixman-cli"
            echo ""
          '';
        };

      }
    );
}
