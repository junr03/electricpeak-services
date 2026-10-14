# shellcheck disable=SC1091
source @FILE_TYPES@

exec @PYTHON@ @PHOTO_WORKFLOW_SCRIPT@ "$@"
