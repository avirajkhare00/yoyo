#!/usr/bin/env bash
set -euo pipefail

target="${1:?usage: use-onnxruntime-asset.sh <target> [destination] [manifest]}"
destination="${2:-${RUNNER_TEMP:-/tmp}/onnxruntime/${target}}"
manifest="${3:-packaging/onnxruntime/assets.json}"

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required to read ${manifest}" >&2
  exit 1
fi

json="$(jq -e --arg target "$target" '.[$target]' "$manifest")"
asset_release_tag="$(jq -r '.asset_release_tag' <<<"$json")"
asset_name="$(jq -r '.asset_name' <<<"$json")"
archive_root="$(jq -r '.archive_root' <<<"$json")"
lib_dir="$(jq -r '.lib_dir' <<<"$json")"
lib_profile="$(jq -r '.lib_profile // empty' <<<"$json")"

repo="${GITHUB_REPOSITORY:-avirajkhare00/yoyo}"
asset_url="https://github.com/${repo}/releases/download/${asset_release_tag}/${asset_name}"
archive_path="${destination}/${asset_name}"
runtime_root="${destination}/${archive_root}"
lib_location="${runtime_root}/${lib_dir}"

emit_var() {
  local key="$1"
  local value="$2"

  if [[ -n "${GITHUB_ENV:-}" ]]; then
    printf '%s=%s\n' "$key" "$value" >> "$GITHUB_ENV"
  fi

  printf '%s=%s\n' "$key" "$value"
}

mkdir -p "$destination"

if [[ ! -d "$runtime_root" ]]; then
  curl -fL --retry 3 --retry-all-errors "$asset_url" -o "$archive_path"
  tar -xzf "$archive_path" -C "$destination"
fi

if [[ ! -d "$lib_location" ]]; then
  echo "Expected ONNX Runtime library directory ${lib_location} after extracting ${asset_url}" >&2
  exit 1
fi

emit_var ORT_RUNTIME_RELEASE_TAG "$asset_release_tag"
emit_var ORT_RUNTIME_ASSET_NAME "$asset_name"
emit_var ORT_RUNTIME_ASSET_URL "$asset_url"
emit_var ORT_RUNTIME_ROOT "$runtime_root"
emit_var ORT_LIB_LOCATION "$lib_location"

if [[ -n "$lib_profile" ]]; then
  emit_var ORT_LIB_PROFILE "$lib_profile"
fi
