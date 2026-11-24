{ lib, rustPlatform }:

rustPlatform.buildRustPackage {
  pname = "transaction-import";
  version = "0.1.0";
  src = lib.cleanSourceWith {
    src = ./.;
    filter = path: type:
      !(builtins.elem (baseNameOf path) [ "target" ".git" ".env" "secrets" "imports" ]);
  };
  cargoLock.lockFile = ./Cargo.lock;
  strictDeps = true;
  meta = {
    description = "Import financial transaction CSV exports into PostgreSQL";
    mainProgram = "transaction-import";
    platforms = lib.platforms.linux;
  };
}
