{ pkgs }:

{
  electricpeak-status-collector = pkgs.rustPlatform.buildRustPackage {
    pname = "electricpeak-status-collector";
    version = "0.1.0";

    src = pkgs.lib.cleanSource ./.;
    cargoLock.lockFile = ./Cargo.lock;

    cargoBuildFlags = [ "-p" "electricpeak-status" ];
    cargoTestFlags = [ "-p" "electricpeak-status" ];
    strictDeps = true;

    installPhase = ''
      runHook preInstall
      install -Dm755 \
        "target/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release/electricpeak-status-collector" \
        "$out/bin/electricpeak-status-collector"
      runHook postInstall
    '';

    meta = {
      description = "Collect the public-safe home server status snapshot";
      mainProgram = "electricpeak-status-collector";
      platforms = pkgs.lib.platforms.linux;
    };
  };
}
