# shellcheck shell=bash
# Shared settings and helpers for the zebratlas ops scripts.
# Every script is DRY-RUN by default and changes something only with --apply.
# Every step checks the current state first, so re-running is safe.

set -euo pipefail

# ---- settings (the one place to change names, paths, ports, limits) ----------
ZA_DOMAIN=zebratlas.org
ZA_ALIASES=www.zebratlas.org
ZA_HOST=94.16.31.179
ZA_USER=zebratlas
ZA_ROOT=/srv/zebratlas            # on the root disk (budget below); block storage = upgrade path (10-storage.sh)
ZA_HOME=$ZA_ROOT/home             # the service user's home (on the volume)
ZA_DATA=$ZA_ROOT/data             # bind-mounted to /data in the containers
ZA_DOCKER=$ZA_ROOT/docker         # rootless Docker data-root (images, volumes)
ZA_APP=$ZA_ROOT/app               # compose file + release.env
ZA_RELEASES=$ZA_ROOT/releases     # uploaded image tarballs
ZA_BACKUPS=$ZA_ROOT/backups       # local DB snapshots (off-host copy via restic)
ZA_ETC=/etc/zebratlas             # root-owned secrets
ZA_ENV=$ZA_ETC/zebratlas.env      # app secrets, root:root 0600
ZA_BACKUP_ENV=$ZA_ETC/backup.env  # restic target + password, root:root 0600
ZA_WEB_PORT=3210                  # 127.0.0.1 only; nginx -> Next.js
ZA_API_PORT=3211                  # 127.0.0.1 only; nginx -> atlas-server
ZA_VOLUME_LABEL=zebratlas         # ext4 label if a dedicated volume is added later
# Disk budget for everything under $ZA_ROOT (data, caches, images, backups), enforced by
# zebratlas-diskguard.timer: warn at WARN %, stop one-off jobs + block releases at 100 %,
# stop the atlas stack at HARD % or when the root fs has less than MIN_FREE left.
ZA_DISK_BUDGET_GB=40
ZA_DISK_WARN_PCT=80
ZA_DISK_HARD_PCT=110
ZA_ROOT_MIN_FREE_GB=25
# Hard cap for EVERYTHING the zebratlas user runs (rootless dockerd + containers + builds).
ZA_SLICE_MEMORY_MAX=3584M   # full data (2026-10-04): API 2.1 GiB idle, ~2.6 GiB under load
ZA_SLICE_MEMORY_HIGH=3200M
ZA_SLICE_CPU_QUOTA=300%           # 3 of 8 cores
ZA_SLICE_TASKS_MAX=4096
ZA_SLICE_IO_WEIGHT=50             # other apps keep the default weight 100
ZA_KEEP_RELEASES=3
# Same file and webroot as the placeholder site set up under D25 (docs/ops/nginx/);
# the certbot renewal config for zebratlas.org uses this webroot.
ZA_NGINX_SITE=/etc/nginx/sites-available/$ZA_DOMAIN
ZA_NGINX_LINK=/etc/nginx/sites-enabled/$ZA_DOMAIN
ZA_ACME_ROOT=/var/www/zebratlas-placeholder
ZA_BASELINE=/root/zebratlas-baseline

OPS_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

# ---- mode ---------------------------------------------------------------------
DRY_RUN=1
ARGS=()
for a in "$@"; do
  case "$a" in
    --apply) DRY_RUN=0 ;;
    --dry-run) DRY_RUN=1 ;;
    *) ARGS+=("$a") ;;
  esac
done
# Callers source this file with "$@" and then run:  set -- ${ARGS[@]+"${ARGS[@]}"}

c_bold=$'\e[1m'; c_dim=$'\e[2m'; c_red=$'\e[31m'; c_grn=$'\e[32m'; c_ylw=$'\e[33m'; c_off=$'\e[0m'
[ -t 1 ] || { c_bold=; c_dim=; c_red=; c_grn=; c_ylw=; c_off=; }

step() { printf '\n%s== %s%s\n' "$c_bold" "$*" "$c_off"; }
ok()   { printf '  %sok%s   %s\n' "$c_grn" "$c_off" "$*"; }
todo() { printf '  %s%s%s %s\n' "$c_ylw" "$([ "$DRY_RUN" = 1 ] && echo 'would' || echo 'doing')" "$c_off" "$*"; }
warn() { printf '  %swarn%s %s\n' "$c_ylw" "$c_off" "$*" >&2; }
die()  { printf '  %sFAIL%s %s\n' "$c_red" "$c_off" "$*" >&2; exit 1; }
mode_banner() {
  if [ "$DRY_RUN" = 1 ]; then
    printf '%sDRY RUN%s: nothing will change. Re-run with --apply to make the changes listed as "would".\n' "$c_ylw" "$c_off"
  else
    printf '%sAPPLY%s: making changes.\n' "$c_red" "$c_off"
  fi
}

# run CMD...: execute only in apply mode; always print it.
run() {
  todo "$(printf '%q ' "$@")"
  [ "$DRY_RUN" = 1 ] || "$@"
}

need_root() { [ "$(id -u)" = 0 ] || die "run as root (the user runs this after review, see DEPLOY-PLAN.md)"; }
have() { command -v "$1" >/dev/null 2>&1; }

za_uid() { id -u "$ZA_USER" 2>/dev/null || true; }

# zdocker ARGS...: the docker CLI against the zebratlas user's ROOTLESS daemon,
# never the system daemon that runs the other apps.
zdocker() {
  local uid; uid=$(za_uid)
  [ -n "$uid" ] || die "user $ZA_USER does not exist yet"
  DOCKER_HOST="unix:///run/user/$uid/docker.sock" docker "$@"
}
zcompose() {
  zdocker compose --project-name zebratlas --project-directory "$ZA_APP" \
    -f "$ZA_APP/compose.yml" --env-file "$ZA_APP/release.env" "$@"
}

# install_file SRC DST MODE OWNER:GROUP — copy if content/mode/owner differ.
install_file() {
  local src=$1 dst=$2 mode=$3 owner=$4
  if [ -f "$dst" ] && cmp -s "$src" "$dst" \
     && [ "$(stat -c '%a %U:%G' "$dst")" = "$mode $owner" ]; then
    ok "$dst unchanged"
    return 0
  fi
  if [ -f "$dst" ]; then
    todo "update $dst ($mode $owner); diff:"
    diff -u "$dst" "$src" | sed 's/^/      /' || true
  else
    todo "create $dst ($mode $owner)"
  fi
  [ "$DRY_RUN" = 1 ] && return 0
  install -D -m "$mode" -o "${owner%%:*}" -g "${owner##*:}" "$src" "$dst"
}

# render TEMPLATE: substitute @VAR@ placeholders from the settings above.
render() {
  sed -e "s|@ZA_DOMAIN@|$ZA_DOMAIN|g" -e "s|@ZA_ALIASES@|$ZA_ALIASES|g" \
      -e "s|@ZA_USER@|$ZA_USER|g" -e "s|@ZA_ROOT@|$ZA_ROOT|g" \
      -e "s|@ZA_DATA@|$ZA_DATA|g" -e "s|@ZA_APP@|$ZA_APP|g" -e "s|@ZA_ENV@|$ZA_ENV|g" \
      -e "s|@ZA_WEB_PORT@|$ZA_WEB_PORT|g" -e "s|@ZA_API_PORT@|$ZA_API_PORT|g" \
      -e "s|@ZA_ACME_ROOT@|$ZA_ACME_ROOT|g" -e "s|@OPS_DIR@|$OPS_DIR|g" \
      -e "s|@ZA_SLICE_MEMORY_MAX@|$ZA_SLICE_MEMORY_MAX|g" -e "s|@ZA_SLICE_MEMORY_HIGH@|$ZA_SLICE_MEMORY_HIGH|g" \
      -e "s|@ZA_SLICE_CPU_QUOTA@|$ZA_SLICE_CPU_QUOTA|g" -e "s|@ZA_SLICE_TASKS_MAX@|$ZA_SLICE_TASKS_MAX|g" \
      -e "s|@ZA_SLICE_IO_WEIGHT@|$ZA_SLICE_IO_WEIGHT|g" "$1"
}

port_in_use() { ss -Htln "sport = :$1" | grep -q .; }
