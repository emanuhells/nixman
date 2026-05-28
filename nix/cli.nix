{ lib
, rustPlatform
, pkg-config
, openssl
, libgit2
, nix
, makeWrapper
}:

rustPlatform.buildRustPackage rec {
  pname = "nixman-cli";
  version = "0.1.0";

  src = lib.cleanSource ../.;

  cargoLock.lockFile = ../Cargo.lock;

  # Build only the CLI crate, skipping Tauri and its WebKitGTK deps.
  cargoBuildFlags = [ "-p" "nixman-cli" ];
  cargoTestFlags  = [ "-p" "nixman-cli" ];

  nativeBuildInputs = [
    pkg-config
    makeWrapper
  ];

  buildInputs = [
    openssl
    libgit2  # required by nixman-core via git2
  ];

  postInstall = ''
    # Ensure nix / nixos-rebuild are available at runtime.
    wrapProgram "$out/bin/nixman" \
      --prefix PATH : ${lib.makeBinPath [ nix ]}
  '';

  meta = with lib; {
    description = "CLI interface for managing NixOS configuration";
    longDescription = ''
      nixman-cli exposes the nixman-core library as a terminal
      interface — no GUI or WebKitGTK required.  Suitable for headless and
      server environments.
    '';
    homepage = "https://github.com/emanuhells/nixman";
    license = licenses.mit;
    maintainers = [];
    platforms = platforms.linux;
    mainProgram = "nixman";
  };
}
