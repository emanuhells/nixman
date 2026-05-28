{ lib
, rustPlatform
, pkg-config
, openssl
, nix
, makeWrapper
}:

rustPlatform.buildRustPackage rec {
  pname = "nixman-mcp";
  version = "0.1.0";

  src = lib.cleanSource ../.;

  cargoLock.lockFile = ../Cargo.lock;

  # Build only the MCP crate.
  cargoBuildFlags = [ "-p" "nixman-mcp" ];
  cargoTestFlags  = [ "-p" "nixman-mcp" ];

  nativeBuildInputs = [
    pkg-config
    makeWrapper
  ];

  buildInputs = [
    openssl
  ];

  postInstall = ''
    # Ensure nix / nixos-rebuild are available at runtime.
    wrapProgram "$out/bin/nixman-mcp" \
      --prefix PATH : ${lib.makeBinPath [ nix ]}
  '';

  meta = with lib; {
    description = "MCP server for nixman — NixOS configuration management via MCP";
    longDescription = ''
      nixman-mcp exposes nixman-core functions as Model Context Protocol (MCP)
      tools. Supports stdio transport (local AI clients) and Streamable HTTP
      transport (remote). Provides tools for reading and modifying NixOS
      configuration options.
    '';
    homepage = "https://github.com/emanuhells/nixman";
    license = licenses.mit;
    maintainers = [];
    platforms = platforms.linux;
    mainProgram = "nixman-mcp";
  };
}
