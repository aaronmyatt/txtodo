# scripts/lab/lib/images.sh — Docker Desktop start-up and the lab images, built only when needed.
# Sourced by scripts/lab/lab.sh.

# What the image is built from. Keep in step with deploy/lab/Dockerfile.dockerignore's allow-list:
# a file in there and not here would change the build without changing the tag.
LAB_BUILD_INPUTS=(Cargo.toml Cargo.lock crates relay apps/desktop/src-tauri build-support corpus
  skills specs deploy/launchd deploy/systemd deploy/lab/Dockerfile deploy/lab/Dockerfile.dockerignore
  deploy/lab/entrypoint.sh deploy/lab/iroh-relay.toml)

# Returns 0 once `docker info` answers. On macOS it starts Docker Desktop first if it is down
# (https://ss64.com/mac/open.html) and waits up to LAB_DOCKER_WAIT seconds.
ensure_docker() {
  if docker info >/dev/null 2>&1; then
    return 0
  fi
  if [ "$(uname -s)" != Darwin ]; then
    return 1
  fi
  log "docker: not running; starting Docker Desktop"
  open -a Docker || return 1
  wait_for "${LAB_DOCKER_WAIT:-180}" docker_answers
}

docker_answers() {
  docker info >/dev/null 2>&1
}

# Removes every lab compose project still around (a run that was killed before its teardown).
# `compose down -p` needs no compose file: it finds the project's containers by label.
# https://docs.docker.com/reference/cli/docker/compose/down/
down_leftovers() {
  local p
  for p in $(docker compose ls -a --format json | jq -r '.[].Name | select(startswith("txlab-"))'); do
    log "docker: removing leftover project $p"
    docker compose -p "$p" down -v --remove-orphans --timeout 5 >/dev/null 2>&1 || true
  done
}

# Content hash of the build inputs in a checkout: tracked files plus new, not-ignored ones.
# `git ls-files -co --exclude-standard`: https://git-scm.com/docs/git-ls-files
source_hash() {
  (
    cd "$1" || exit 1
    git ls-files -co --exclude-standard -z -- "${LAB_BUILD_INPUTS[@]}" |
      xargs -0 shasum 2>/dev/null
  ) | shasum | cut -c1-12
}

# The iroh-relay version a Cargo.lock pins; the lab relay is built at exactly that version.
iroh_relay_version() {
  awk '/^name = "iroh-relay"$/ { getline; gsub(/version = |"/, ""); print; exit }' "$1"
}

# build_image <context dir> <tag> <log file>
build_image() {
  local ctx=$1 tag=$2 build_log=$3 relay_version
  relay_version=$(iroh_relay_version "$ctx/Cargo.lock")
  log "image: building $tag (log: $build_log)"
  # BuildKit is what reads the cache mounts and the <Dockerfile>.dockerignore.
  # https://docs.docker.com/build/buildkit/
  if ! DOCKER_BUILDKIT=1 docker build -f "$LAB_DEPLOY/Dockerfile" --target device \
    --build-arg IROH_RELAY_VERSION="$relay_version" --label txtodo-lab=1 \
    -t "$tag" "$ctx" >"$build_log" 2>&1; then
    log "image: build of $tag failed; last lines:"
    tail -20 "$build_log"
    return 1
  fi
  log "image: built $tag"
}

# Sets LAB_IMAGE to this checkout's image, building it if no image has its source hash yet.
ensure_current_image() {
  local tag
  # An image already built (say, before a fix) instead of this checkout's: compare old and new.
  if [ -n "${LAB_USE_IMAGE:-}" ]; then
    docker image inspect "$LAB_USE_IMAGE" >/dev/null 2>&1 || return 1
    log "image: $LAB_USE_IMAGE (LAB_USE_IMAGE)"
    export LAB_IMAGE=$LAB_USE_IMAGE
    return 0
  fi
  tag="txtodo-lab:src-$(source_hash "$LAB_ROOT")"
  if docker image inspect "$tag" >/dev/null 2>&1; then
    log "image: $tag (up to date)"
  else
    build_image "$LAB_ROOT" "$tag" "$RUN_DIR/build-current.log" || return 1
  fi
  export LAB_IMAGE=$tag
}

# The newest release tag whose crates/ differ from HEAD's: pairing it with HEAD tests something.
# LAB_OLD_REF overrides. `--merged HEAD` keeps tags from other branches out.
# https://git-scm.com/docs/git-tag
pick_old_ref() {
  if [ -n "${LAB_OLD_REF:-}" ]; then
    printf '%s\n' "$LAB_OLD_REF"
    return 0
  fi
  local head tag
  head=$(git -C "$LAB_ROOT" rev-parse HEAD:crates)
  for tag in $(git -C "$LAB_ROOT" tag -l 'v*' --sort=-v:refname --merged HEAD); do
    if [ "$(git -C "$LAB_ROOT" rev-parse "$tag:crates")" != "$head" ]; then
      printf '%s\n' "$tag"
      return 0
    fi
  done
  return 1
}

# Sets LAB_OLD_IMAGE (and LAB_OLD_REF_USED) to an image of the old release, building it once.
# The source comes from `git archive` (https://git-scm.com/docs/git-archive); the lab's own
# entrypoint and relay config come from this checkout, since old releases have none.
ensure_old_image() {
  local ref commit tag ctx p paths=()
  ref=$(pick_old_ref) || return 1
  commit=$(git -C "$LAB_ROOT" rev-parse --short "$ref^{commit}")
  tag="txtodo-lab:${ref//[^A-Za-z0-9_.-]/-}-$commit"
  if ! docker image inspect "$tag" >/dev/null 2>&1; then
    ctx=$(mktemp -d "$RUN_DIR/old-src.XXXXXX")
    for p in Cargo.toml Cargo.lock crates relay apps/desktop/src-tauri build-support corpus skills \
      specs deploy/launchd deploy/systemd; do
      if git -C "$LAB_ROOT" cat-file -e "$ref:$p" 2>/dev/null; then
        paths+=("$p")
      fi
    done
    git -C "$LAB_ROOT" archive "$ref" -- "${paths[@]}" | tar -x -C "$ctx"
    mkdir -p "$ctx/deploy/lab"
    cp "$LAB_DEPLOY/entrypoint.sh" "$LAB_DEPLOY/iroh-relay.toml" "$ctx/deploy/lab/"
    build_image "$ctx" "$tag" "$RUN_DIR/build-$ref.log" || {
      rm -rf "$ctx"
      return 1
    }
    rm -rf "$ctx"
  else
    log "image: $tag (up to date)"
  fi
  export LAB_OLD_IMAGE=$tag LAB_OLD_REF_USED=$ref
}

# Keeps the newest 3 source images and the newest 2 release images; removes older ones.
# `docker image ls` lists newest first. https://docs.docker.com/reference/cli/docker/image/ls/
prune_images() {
  local t
  docker image ls txtodo-lab --format '{{.Tag}}' | grep '^src-' | tail -n +4 |
    while read -r t; do docker image rm "txtodo-lab:$t" >/dev/null 2>&1 || true; done
  docker image ls txtodo-lab --format '{{.Tag}}' | grep '^v' | tail -n +3 |
    while read -r t; do docker image rm "txtodo-lab:$t" >/dev/null 2>&1 || true; done
}
