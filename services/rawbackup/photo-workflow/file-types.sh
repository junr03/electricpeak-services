# The complete file-type contract for the photo workflow. Every importer,
# backup, verification, reconciliation, and inventory job sources this file.
# Files whose final extension is not listed here are intentionally invisible
# to the workflow. AppleDouble metadata files (._*) are also invisible even
# when their companion filename ends in a managed extension. Neither kind is
# copied, compared, counted, or treated as a conflict.
readonly PHOTO_RAW_EXTENSIONS="arw cr2 cr3 dng nef orf raf rw2"
readonly PHOTO_SIDECAR_EXTENSIONS="acr photo-edit xmp"
export PHOTO_RAW_EXTENSIONS PHOTO_SIDECAR_EXTENSIONS

photo_casefold_extension_pattern() {
  local extension="$1"
  local pattern="" character lower upper
  local index

  for ((index = 0; index < ${#extension}; index += 1)); do
    character="${extension:index:1}"
    lower="${character,,}"
    upper="${character^^}"
    if [ "$lower" != "$upper" ]; then
      pattern+="[$lower$upper]"
    else
      pattern+="$character"
    fi
  done
  printf '*.%s' "$pattern"
}

PHOTO_RAW_PATTERNS=()
PHOTO_SIDECAR_PATTERNS=()
PHOTO_RSYNC_RAW_FILTERS=("--exclude=._*" "--include=*/")
PHOTO_RSYNC_SIDECAR_FILTERS=("--exclude=._*" "--include=*/")
PHOTO_RSYNC_ALL_FILTERS=("--exclude=._*" "--include=*/")
PHOTO_RCLONE_RAW_FILTERS=("--filter=- ._*")
PHOTO_RCLONE_SIDECAR_FILTERS=("--filter=- ._*")
PHOTO_RCLONE_ALL_FILTERS=("--filter=- ._*")
PHOTO_FIND_RAW_EXPRESSION=(-false)
PHOTO_FIND_SIDECAR_EXPRESSION=(-false)
PHOTO_FIND_ALL_EXPRESSION=(-false)

for photo_extension in $PHOTO_RAW_EXTENSIONS; do
  photo_pattern="$(photo_casefold_extension_pattern "$photo_extension")"
  PHOTO_RAW_PATTERNS+=("$photo_pattern")
  PHOTO_RSYNC_RAW_FILTERS+=("--include=$photo_pattern")
  PHOTO_RSYNC_ALL_FILTERS+=("--include=$photo_pattern")
  PHOTO_RCLONE_RAW_FILTERS+=("--filter=+ $photo_pattern")
  PHOTO_RCLONE_ALL_FILTERS+=("--filter=+ $photo_pattern")
  PHOTO_FIND_RAW_EXPRESSION+=(-o -iname "*.$photo_extension")
  PHOTO_FIND_ALL_EXPRESSION+=(-o -iname "*.$photo_extension")
done

for photo_extension in $PHOTO_SIDECAR_EXTENSIONS; do
  photo_pattern="$(photo_casefold_extension_pattern "$photo_extension")"
  PHOTO_SIDECAR_PATTERNS+=("$photo_pattern")
  PHOTO_RSYNC_SIDECAR_FILTERS+=("--include=$photo_pattern")
  PHOTO_RSYNC_ALL_FILTERS+=("--include=$photo_pattern")
  PHOTO_RCLONE_SIDECAR_FILTERS+=("--filter=+ $photo_pattern")
  PHOTO_RCLONE_ALL_FILTERS+=("--filter=+ $photo_pattern")
  PHOTO_FIND_SIDECAR_EXPRESSION+=(-o -iname "*.$photo_extension")
  PHOTO_FIND_ALL_EXPRESSION+=(-o -iname "*.$photo_extension")
done
unset photo_extension photo_pattern

PHOTO_RSYNC_RAW_FILTERS+=("--exclude=*")
PHOTO_RSYNC_SIDECAR_FILTERS+=("--exclude=*")
PHOTO_RSYNC_ALL_FILTERS+=("--exclude=*")
PHOTO_RCLONE_RAW_FILTERS+=("--filter=- **")
PHOTO_RCLONE_SIDECAR_FILTERS+=("--filter=- **")
PHOTO_RCLONE_ALL_FILTERS+=("--filter=- **")

photo_find_files() {
  local root="$1"
  local kind="$2"
  shift 2
  local expression_name

  case "$kind" in
    raw) expression_name=PHOTO_FIND_RAW_EXPRESSION ;;
    sidecar) expression_name=PHOTO_FIND_SIDECAR_EXPRESSION ;;
    all) expression_name=PHOTO_FIND_ALL_EXPRESSION ;;
    *)
      printf 'Unknown photo file kind: %s\n' "$kind" >&2
      return 2
      ;;
  esac

  local -n expression="$expression_name"
  find "$root" -type f ! -name '._*' \( "${expression[@]}" \) "$@"
}

photo_name_has_extension() {
  local name="${1##*/}"
  local extensions="$2"
  local extension
  case "$name" in
    ._*) return 1 ;;
  esac
  [ "$name" != "${name%.*}" ] || return 1
  extension="${name##*.}"
  extension="${extension,,}"
  case " $extensions " in
    *" $extension "*) return 0 ;;
    *) return 1 ;;
  esac
}

photo_name_is_raw() {
  photo_name_has_extension "$1" "$PHOTO_RAW_EXTENSIONS"
}

photo_name_is_sidecar() {
  photo_name_has_extension "$1" "$PHOTO_SIDECAR_EXTENSIONS"
}

photo_name_is_managed() {
  photo_name_is_raw "$1" || photo_name_is_sidecar "$1"
}
