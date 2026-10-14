{ pkgs, renamePicture, localRoot, internxtRemotePath,
  internxtConfig, internxtEmailSecret, internxtPasswordSecret }:

let
  progressCommand = "${pkgs.python3}/bin/python3 ${./progress.py}";
  fileTypes = "${./file-types.sh}";
  renderScript = path: from: to:
    builtins.replaceStrings from to (builtins.readFile path);

  importer = pkgs.writeShellApplication {
    name = "photo-workflow-importer";
    runtimeInputs = with pkgs; [ coreutils rsync util-linux ] ++ [ renamePicture ];
    text = renderScript
      ./importer.sh
      [ "@PYTHON@" "@PHOTO_WORKFLOW_SCRIPT@" "@FILE_TYPES@" ]
      [ "${pkgs.python3}/bin/python3" "${./import.py}" fileTypes ];
  };

  importRunner = pkgs.writeShellApplication {
    name = "photo-workflow-import-runner";
    runtimeInputs = with pkgs; [ coreutils findutils gawk gnugrep jq rsync util-linux ];
    text = renderScript
      ./import-runner.sh
      [ "@LOCAL_ROOT@" "@IMPORTER@" "@GREP@" "@PROGRESS@" "@FILE_TYPES@" ]
      [ localRoot "${importer}/bin/photo-workflow-importer" "${pkgs.gnugrep}/bin/grep" progressCommand fileTypes ];
  };

  externalSync = pkgs.writeShellApplication {
    name = "photo-workflow-external-sync";
    runtimeInputs = with pkgs; [ coreutils diffutils findutils gnused gnugrep rsync util-linux ];
    text = renderScript ./external-sync.sh [ "@PROGRESS@" "@FILE_TYPES@" ] [ progressCommand fileTypes ];
  };

  reconciler = pkgs.writeShellApplication {
    name = "photo-workflow-reconcile";
    runtimeInputs = with pkgs; [ coreutils findutils gawk gnugrep jq rclone rsync util-linux systemd ];
    text = renderScript ./reconcile.sh [ "@PROGRESS@" "@FILE_TYPES@" ] [ progressCommand fileTypes ];
  };

  internxtConfigurator = pkgs.writeShellApplication {
    name = "photo-workflow-configure-internxt";
    runtimeInputs = with pkgs; [ coreutils gnugrep rclone ];
    text = renderScript
      ./configure-internxt.sh
      [ "@INTERNXT_CONFIG@" "@INTERNXT_EMAIL_SECRET@" "@INTERNXT_PASSWORD_SECRET@" ]
      [ internxtConfig internxtEmailSecret internxtPasswordSecret ];
  };

  internxtBackup = pkgs.writeShellApplication {
    name = "photo-workflow-internxt-backup";
    runtimeInputs = with pkgs; [ coreutils rclone util-linux ];
    text = renderScript
      ./internxt-backup.sh
      [ "@LOCAL_ROOT@" "@INTERNXT_REMOTE_PATH@" "@INTERNXT_CONFIG@" "@PROGRESS@" "@FILE_TYPES@" ]
      [ localRoot internxtRemotePath internxtConfig progressCommand fileTypes ];
  };

  internxtCheck = pkgs.writeShellApplication {
    name = "photo-workflow-internxt-check";
    runtimeInputs = with pkgs; [ coreutils rclone util-linux ];
    text = renderScript
      ./internxt-check.sh
      [ "@LOCAL_ROOT@" "@INTERNXT_REMOTE_PATH@" "@INTERNXT_CONFIG@" "@PROGRESS@" "@FILE_TYPES@" ]
      [ localRoot internxtRemotePath internxtConfig progressCommand fileTypes ];
  };

  internxtSidecarBackup = pkgs.writeShellApplication {
    name = "photo-workflow-internxt-sidecar-backup";
    runtimeInputs = with pkgs; [ coreutils rclone util-linux ];
    text = renderScript
      ./internxt-sidecar-backup.sh
      [ "@LOCAL_ROOT@" "@INTERNXT_REMOTE_PATH@" "@INTERNXT_CONFIG@" "@PROGRESS@" "@FILE_TYPES@" ]
      [ localRoot internxtRemotePath internxtConfig progressCommand fileTypes ];
  };
in
{
  inherit importer importRunner externalSync reconciler internxtConfigurator
    internxtBackup internxtCheck internxtSidecarBackup;
}
