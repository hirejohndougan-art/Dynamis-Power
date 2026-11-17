#!/bin/sh
# Build dynamis-power-service-<version>-1.<dist>.<arch>.rpm (Fedora, RHEL
# and relatives) from this workspace. Needs rpmbuild and systemd-rpm-macros.
#
#   cd daemon && ./packaging/build-rpm.sh            # builds, then packages
#   ./packaging/build-rpm.sh --no-build              # package existing binaries
#
# Output goes to target/rpm/.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)

[ "${1:-}" = "--no-build" ] || (cd "$root" && cargo build --release --locked)

version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
"$here/stage.sh" "$work/stage"

rpmbuild -bb \
    --define "_topdir $work/rpmbuild" \
    --define "pkgversion $version" \
    --define "stage $work/stage" \
    --define "licensefile $root/../LICENSE" \
    --define "changelogfile $root/../CHANGELOG.md" \
    "$here/rpm/dynamis-power-service.spec"

out="$root/target/rpm"
mkdir -p "$out"
find "$work/rpmbuild/RPMS" -name '*.rpm' -exec cp {} "$out/" \;
ls "$out"/*.rpm
