#!/usr/bin/env bash
# Download the CLAP model files (task T5.1, ADR-026). Weights are never in git.
#
# Model: LAION `clap-htsat-unfused` (Apache-2.0), ONNX export by
# `Xenova/clap-htsat-unfused`, pinned to one Hugging Face commit. Files:
#   audio_model_quantized.onnx  audio tower, int8 dynamic quantization
#   text_model.onnx             text tower, fp32
#   tokenizer.json              RoBERTa byte-level BPE (HF tokenizers format)
#   preprocessor_config.json    front-end parameters (reference only)
#   config.json                 model config (reference only)
#
# Destination: ${SONAR_MODELS_DIR:-<repo>/models}/clap-htsat-unfused/
#
# Every file is checked against a pinned SHA-256. A file that is already
# present and verifies is skipped (idempotent); a present file that does not
# verify is deleted and downloaded again; a download that does not verify is
# deleted and the script fails. Downloads go to `<file>.part` first, so an
# interrupted run never leaves a half-written file under the final name.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MODEL="clap-htsat-unfused"
REPO="Xenova/clap-htsat-unfused"
REV="c28f2883575e590e04d3146ff0713c2448d691ba"
DEST="${SONAR_MODELS_DIR:-$ROOT/models}/$MODEL"

# remote path | local name | bytes | sha256
FILES="
onnx/audio_model_quantized.onnx|audio_model_quantized.onnx|34301667|3fcff2c8824e7bcb83a983f2a49edab3b60cbcf4872ac70efee517355173bd1f
onnx/text_model.onnx|text_model.onnx|501513769|6df7c51f2d78e236a03f2b816e613d538c815639d13644fe7c2124f439da9648
tokenizer.json|tokenizer.json|2108774|dc239041d98de27ffc3975473a1a23e3db4c937b23c138c38bbc66588bd247e5
preprocessor_config.json|preprocessor_config.json|541|9739f58296aa6f9ac18008fd0150fb2649bc554985fbde86d0a4041c882ac753
config.json|config.json|699|39c6d90fe29cf2cce650dd5c92c38a1e35b130d9ce0bb98585222ad687ad979b
"

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  fi
}

command -v curl >/dev/null 2>&1 || { echo "fetch-models: curl not found" >&2; exit 2; }
if ! command -v shasum >/dev/null 2>&1 && ! command -v sha256sum >/dev/null 2>&1; then
  echo "fetch-models: need shasum or sha256sum" >&2
  exit 2
fi
mkdir -p "$DEST"

failed=0
while IFS='|' read -r remote name bytes sha; do
  [ -n "$remote" ] || continue
  out="$DEST/$name"
  if [ -f "$out" ]; then
    if [ "$(sha256_of "$out")" = "$sha" ]; then
      echo "ok       $name (already verified)"
      continue
    fi
    echo "mismatch $name: existing file fails SHA-256, downloading again"
    rm -f "$out"
  fi
  url="https://huggingface.co/$REPO/resolve/$REV/$remote"
  echo "fetch    $name ($bytes bytes)"
  rm -f "$out.part"
  if ! curl -fL --proto '=https' --tlsv1.2 --retry 3 --retry-delay 2 \
      --silent --show-error -o "$out.part" "$url"; then
    rm -f "$out.part"
    echo "FAILED   $name: download error ($url)" >&2
    failed=1
    continue
  fi
  got="$(sha256_of "$out.part")"
  if [ "$got" != "$sha" ]; then
    rm -f "$out.part"
    echo "FAILED   $name: SHA-256 mismatch (expected $sha, got $got); file deleted" >&2
    failed=1
    continue
  fi
  mv "$out.part" "$out"
  echo "ok       $name (verified)"
done <<EOF
$FILES
EOF

if [ "$failed" -ne 0 ]; then
  echo "fetch-models: some files failed; nothing unverified was kept" >&2
  exit 1
fi
echo "fetch-models: $MODEL ready in $DEST"
