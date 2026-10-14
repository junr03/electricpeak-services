config_file="@INTERNXT_CONFIG@"
config_dir="$(dirname "$config_file")"

# Internxt's post-config login stores the mnemonic and OAuth token.
# Preserve a complete config so boot-time restarts do not discard them.
if [ -s "$config_file" ] \
  && grep -q '^\[internxt\]$' "$config_file" \
  && grep -q '^mnemonic = .\+$' "$config_file" \
  && grep -q '^token = .\+$' "$config_file"; then
  exit 0
fi

temp_file="$(mktemp "$config_dir/rclone.conf.XXXXXX")"
trap 'rm -f "$temp_file"' EXIT

email="$(cat @INTERNXT_EMAIL_SECRET@)"
password="$(rclone obscure - < @INTERNXT_PASSWORD_SECRET@)"
rclone config create internxt internxt \
  email "$email" \
  pass "$password" \
  skip_hash_validation true \
  upload_concurrency 2 \
  upload_cutoff 100Mi \
  chunk_size 100Mi \
  --no-obscure \
  --no-output \
  --config "$temp_file"
unset password
chmod 600 "$temp_file"
mv -f "$temp_file" "$config_file"
