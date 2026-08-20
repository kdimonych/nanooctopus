#!/usr/bin/env bash
set -euo pipefail

readonly TESTABLE_FEATURE_SETS=(
  ""
  "std"
  "std,log"
  "bench_masker"
  "std,bench_masker"
)

readonly FEATURE_SETS=(
  ""
  "std"
  "std,log"
  "bench_masker"
  "std,bench_masker"
  "defmt"
)

simple_run(){
  local work_dir="$1"
  local cargo_subcommand="$2"
  shift 2

  local -a extra_args=("$@")

  # If work_dir is not current directory, print it out for clarity
  if [ "$work_dir" != "." ]; then
    echo "Command: (cd \"$work_dir\" && cargo $cargo_subcommand $@)"
  else
    echo "Command: cargo $cargo_subcommand $@"
  fi

  if ((${#extra_args[@]} == 0)); then
    (
      cd "$work_dir"
      cargo "$cargo_subcommand"
    )
  else
    (
      cd "$work_dir"
      cargo "$cargo_subcommand" "${extra_args[@]}"
    )
  fi
}

run_for_features() {
  local array_name="$1"
  local work_dir="$2"
  local cargo_subcommand="$3"
  shift 3

  # macOS default Bash (3.2) does not support namerefs (local -n).
  # Copy the target array by name into a local array via eval.
  local -a features_ref
  eval "features_ref=(\"\${${array_name}[@]}\")"

  for feature_set in "${features_ref[@]}"; do
    if [[ -n "$feature_set" ]]; then
      echo "Running with features: $feature_set"
      simple_run "$work_dir" "$cargo_subcommand" --no-default-features --features "$feature_set" "$@"
    else
      echo "Running with no features"
      simple_run "$work_dir" "$cargo_subcommand" --no-default-features "$@"
    fi
  done
}

case "${1:-all}" in
  list)
    printf '%s\n' "${FEATURE_SETS[@]}"
    ;;
  build)
    run_for_features FEATURE_SETS . build
    simple_run ./demos/raspberry_pico_w build
    ;;
  clippy)
    run_for_features FEATURE_SETS . clippy --no-deps --workspace
    simple_run ./demos/raspberry_pico_w clippy --no-deps
    ;;
  test)
    run_for_features TESTABLE_FEATURE_SETS . test --workspace
    ;;
  bench)
    simple_run . bench --workspace --features bench_masker
    ;;
  all)
    #  build
    run_for_features FEATURE_SETS . build
    simple_run ./demos/raspberry_pico_w build
    #  clippy
    run_for_features FEATURE_SETS . clippy --no-deps
    simple_run ./demos/raspberry_pico_w clippy --no-deps
    #  test
    run_for_features TESTABLE_FEATURE_SETS . test --workspace
    ;;
  *)
    echo "usage: $0 [list|build|clippy|test|all]" >&2
    exit 1
    ;;
esac
