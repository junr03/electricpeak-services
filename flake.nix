{
  description = "Electricpeak application packages and development environments";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    gallatin = {
      url = "github:junr03/gallatin?rev=e6ca46f2fe0c02a396142ffbfd1864f517453b85";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, gallatin, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
    in {
      checks.${system} = {
        rawbackup-api-contract = pkgs.runCommand "rawbackup-api-contract" {
          nativeBuildInputs = [ pkgs.python3Packages.openapi-spec-validator ];
        } ''
          openapi-spec-validator ${./services/rawbackup/contracts/rawbackup/v1/openapi.yaml}
          touch $out
        '';
      };
      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [ cargo clippy rustc rustfmt python3 rsync ];
      };
    };
}
