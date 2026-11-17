#!/usr/bin/env bash
# Design-token gate (docs/DESIGN.md §5): feature code must take every colour,
# space, radius and font from `Theme` (app/Sonar/DesignSystem/). Fails on
# literals in app/Sonar outside DesignSystem/. There is no exemption list;
# a single line can opt out with a trailing `// design-lint:allow <reason>`.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

NAMED='red|orange|yellow|green|mint|teal|cyan|blue|indigo|purple|pink|brown|gray|black|white'
PATTERNS=(
  # literal SwiftUI colours: Color.red, .foregroundStyle(.orange), .red.opacity(…)
  "Color\.(${NAMED})\b"
  "\.(foregroundStyle|foregroundColor|background|tint|fill|stroke|border)\(\s*\.(${NAMED})\b"
  "Color\(\s*(red|white|hue|\.sRGB|\.displayP3)"
  # literal AppKit colours (semantic label/control colours are fine)
  "NSColor\.(system[A-Z][A-Za-z]+|${NAMED})\b"
  "NSColor\(\s*(red|white|srgbRed|calibratedRed|deviceRed|hue)"
  # hex colours
  "#[0-9A-Fa-f]{6}\b"
  # literal spacing / padding / radius / font size
  "\.padding\(\s*(\.[A-Za-z]+\s*,\s*)?[0-9]"
  "\.padding\(\s*\[[^]]*\]\s*,\s*[0-9]"
  "spacing:\s*[1-9]"
  "cornerRadius:\s*[0-9]"
  "\.font\(\s*\.system\(\s*size:"
  "ofSize:\s*[0-9]"
)

fail=0
while IFS= read -r file; do
  for pattern in "${PATTERNS[@]}"; do
    if hits=$(grep -nE "$pattern" "$file" | grep -v "design-lint:allow"); then
      while IFS= read -r hit; do printf '%s:%s\n' "$file" "$hit"; done <<<"$hits"
      fail=1
    fi
  done
done < <(find app/Sonar -name '*.swift' -not -path 'app/Sonar/DesignSystem/*' | sort)

if [[ $fail -ne 0 ]]; then
  printf '\n✗ design-lint: use Theme tokens (docs/DESIGN.md §2, §5)\n' >&2
  exit 1
fi
printf '✓ design-lint\n'
