#!/usr/bin/env bash
# Verify that every user-visible package version matches a vX.Y.Z release tag.
set -euo pipefail

version="${1:?usage: verify-release-version.sh X.Y.Z}"

cargo_version="$(awk -F '"' '/^version = / { print $2; exit }' backend/Cargo.toml)"
npm_version="$(awk -F '"' '/"version"/ { print $4; exit }' frontend/package.json)"
chart_version="$(awk '/^version:/ { print $2; exit }' deploy/chart/Chart.yaml)"

for component in cargo npm chart; do
    case "$component" in
        cargo) actual="$cargo_version" ;;
        npm) actual="$npm_version" ;;
        chart) actual="$chart_version" ;;
    esac
    if [ "$actual" != "$version" ]; then
        echo "Version mismatch: tag ($version) != $component ($actual)" >&2
        exit 1
    fi
done

echo "All package versions match $version"
