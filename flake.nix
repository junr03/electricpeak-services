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
      workflows = self.lib.mkPhotoWorkflowPackages {
        inherit pkgs;
        renamePicture = pkgs.callPackage "${gallatin}/rename-picture.nix" { };
        localRoot = "/mnt/data/photos";
        internxtRemotePath = "photos";
        internxtConfig = "/var/lib/photo-workflow/rclone.conf";
        internxtEmailSecret = "/run/secrets/internxt-email";
        internxtPasswordSecret = "/run/secrets/internxt-password";
      };
    in {
      lib.mkPhotoWorkflowPackages = import ./services/rawbackup/photo-workflow/packages.nix;
      packages.${system}.photo-workflow = pkgs.symlinkJoin {
        name = "photo-workflow";
        paths = builtins.attrValues workflows;
      };
      checks.${system} = self.packages.${system} // {
        rawbackup-api-contract = pkgs.runCommand "rawbackup-api-contract" {
          nativeBuildInputs = [ pkgs.python3Packages.openapi-spec-validator ];
        } ''
          openapi-spec-validator ${./services/rawbackup/contracts/rawbackup/v1/openapi.yaml}
          touch $out
        '';
        rawbackup-python = pkgs.runCommand "rawbackup-python-tests" {
          nativeBuildInputs = [ pkgs.python3 pkgs.bash pkgs.rsync pkgs.rclone ];
        } ''
          cp -r ${./services/rawbackup} rawbackup
          chmod -R u+w rawbackup
          python -m unittest discover -s rawbackup/photo-workflow/tests -v
          touch $out
        '';
      };
      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [ cargo clippy rustc rustfmt python3 rsync ];
      };
    };
}
