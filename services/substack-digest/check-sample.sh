#!/usr/bin/env bash
set -euo pipefail
pdf=$1
# 91.8 x 163.2 mm in PDF points, with rounding tolerance.
pdfinfo -f 1 -l 1 "$pdf" | awk '
  /Page +1 size:/ { if ($4 < 259 || $4 > 262 || $6 < 461 || $6 > 464) exit 1; size = 1 }
  /Pages:/ { if ($2 < 3) exit 1; pages = 1 }
  END { if (!size || !pages) exit 1 }
'
text=$(pdftotext -layout "$pdf" -)
[[ $(grep -c 'A good reading layout' <<< "$text") -eq 16 ]]
grep -q 'Original article' <<< "$text"
printf 'Sample PDF dimensions and complete article text verified.\n'
