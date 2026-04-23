#!/usr/bin/env bash
set -euo pipefail

readonly FEATURE_SETS=(
  "tokio_impl"
  "tokio_impl,ws"
  "tokio_impl,log"
  "tokio_impl,ws,log"
  "embassy_impl"
  "embassy_impl,proto-ipv6"
  "embassy_impl,ws"
  "embassy_impl,ws,proto-ipv6"
  "embassy_impl,defmt"
  "embassy_impl,defmt,proto-ipv6"
  "embassy_impl,ws,defmt"
  "embassy_impl,ws,defmt,proto-ipv6"
)

run_for_all_features() {
  local cargo_subcommand="$1"
  shift

  local -a extra_args=("$@")

  for feature_set in "${FEATURE_SETS[@]}"; do
    printf '\n==> %s [%s]\n' "$cargo_subcommand" "$feature_set"
    if ((${#extra_args[@]} == 0)); then
      # Trace command line for better visibility when running with `all`.
      echo "Command: cargo $cargo_subcommand --no-default-features --features $feature_set"
      cargo "$cargo_subcommand" --no-default-features --features "$feature_set"
    else
      # Trace command line for better visibility when running with `all`.
      echo "Command: cargo $cargo_subcommand --no-default-features --features $feature_set ${extra_args[*]}"
      cargo "$cargo_subcommand" --no-default-features --features "$feature_set" "${extra_args[@]}"
    fi
  done
}

case "${1:-all}" in
  list)
    printf '%s\n' "${FEATURE_SETS[@]}"
    ;;
  build)
    run_for_all_features build
    ;;
  clippy)
    run_for_all_features clippy --no-deps
    ;;
  test)
    run_for_all_features test
    ;;
  all)
    run_for_all_features build
    run_for_all_features clippy --no-deps
    run_for_all_features test
    ;;
  *)
    echo "usage: $0 [list|build|clippy|test|all]" >&2
    exit 1
    ;;
esac
