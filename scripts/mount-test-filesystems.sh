#!/usr/bin/env bash
# Create and loop-mount small ext4, btrfs and xfs filesystems for the
# privileged graft-fs tests. Linux only; must run as root.
#
#   sudo scripts/mount-test-filesystems.sh <work-dir> <uid:gid>
#
# Images go in <work-dir>/img, mounts in <work-dir>/mnt/<fs>, owned by
# <uid:gid>. Prints the GRAFT_TEST_FS_MOUNTS value, and appends it to
# $GITHUB_ENV when that is set.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <work-dir> <uid:gid>" >&2
  exit 2
fi
if [[ $(id -u) -ne 0 ]]; then
  echo "$0: must run as root" >&2
  exit 1
fi

work=$1
owner=$2
mkdir -p "$work/img" "$work/mnt"

mounts=()
for fs in ext4 btrfs xfs; do
  img="$work/img/$fs.img"
  mnt="$work/mnt/$fs"
  # 512 MiB satisfies every mkfs minimum (xfs needs ~300 MiB). Benchmarks
  # set SIZE for room to work.
  truncate -s "${SIZE:-512M}" "$img"
  case $fs in
    ext4) mkfs.ext4 -q -F "$img" ;;
    btrfs) mkfs.btrfs -q -f "$img" ;;
    # reflink=1 is the default in current xfsprogs; be explicit.
    xfs) mkfs.xfs -q -f -m reflink=1 "$img" ;;
  esac
  mkdir -p "$mnt"
  mount -o loop "$img" "$mnt"
  chown "$owner" "$mnt"
  mounts+=("$fs=$mnt")
done

value=$(IFS=,; echo "${mounts[*]}")
echo "GRAFT_TEST_FS_MOUNTS=$value"
if [[ -n ${GITHUB_ENV:-} ]]; then
  echo "GRAFT_TEST_FS_MOUNTS=$value" >> "$GITHUB_ENV"
fi
