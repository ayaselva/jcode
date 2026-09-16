#!/usr/bin/env bash
# Werk de PC001 Exa-koppeling van jcode bij: upstream ophalen, onze branch
# rebasen, guardrails draaien, bouwen/publiceren en de launcher-wrapper opnieuw
# toepassen.
#
# Gebruik:
#   pc001/upgrade.sh                    # fetch + rebase + guardrails + build + install
#   pc001/upgrade.sh --no-rebase        # alleen bouwen/publiceren
#   pc001/upgrade.sh --skip-guardrails  # guardrails overslaan
#
# Upstream: https://github.com/1jehuang/jcode.git (remote `upstream`).
# Onze branch staat op `myplace` (Forgejo) en, als je rechten hebt, op de fork
# https://github.com/ayaselva/jcode.git (remote `fork`).
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "${here}/.." && pwd)
upstream_url="https://github.com/1jehuang/jcode.git"

REBASE=1
GUARDRAILS=1
for arg in "$@"; do
  case "$arg" in
    --no-rebase) REBASE=0 ;;
    --skip-guardrails) GUARDRAILS=0 ;;
    -h|--help)
      sed -n '2,14p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "upgrade.sh: onbekende optie '$arg'" >&2
      exit 2
      ;;
  esac
done

log() { printf '▸ %s\n' "$*"; }

ensure_remote() { # $1=name, $2=url
  if git -C "${repo}" remote get-url "$1" >/dev/null 2>&1; then
    return 0
  fi
  log "remote $1 toevoegen: $2"
  git -C "${repo}" remote add "$1" "$2"
}

ensure_remote upstream "${upstream_url}"

if [[ -n "$(git -C "${repo}" status --porcelain)" ]]; then
  echo "upgrade.sh: werkboom ${repo} is niet schoon; commit of stash eerst" >&2
  git -C "${repo}" status --short >&2
  exit 1
fi

branch=$(git -C "${repo}" rev-parse --abbrev-ref HEAD)
log "fetch van alle remotes (branch ${branch})"
git -C "${repo}" fetch --all --prune

if (( REBASE )); then
  upstream_ref=$(git -C "${repo}" rev-parse --abbrev-ref 'upstream/HEAD' 2>/dev/null || true)
  if [[ -z "${upstream_ref}" || "${upstream_ref}" == "upstream/HEAD" ]]; then
    upstream_ref="upstream/main"
    git -C "${repo}" rev-parse --verify -q "${upstream_ref}" >/dev/null \
      || upstream_ref="upstream/master"
  fi
  if git -C "${repo}" rev-parse --verify -q "${upstream_ref}" >/dev/null; then
    if git -C "${repo}" merge-base --is-ancestor "${upstream_ref}" HEAD; then
      log "${upstream_ref} zit al in ${branch}; niets te rebasen"
    else
      log "rebasen van ${branch} op ${upstream_ref}"
      if ! git -C "${repo}" rebase "${upstream_ref}"; then
        git -C "${repo}" rebase --abort || true
        echo "upgrade.sh: rebase gaf conflicten en is afgebroken; los dat handmatig op:" >&2
        echo "  git -C ${repo} rebase ${upstream_ref}" >&2
        exit 1
      fi
    fi
  else
    log "geen upstream-branch gevonden; alleen gefetcht"
  fi
fi

# install.sh draait de guardrails, de build, de publicatie, de wrapper en de config.
if (( GUARDRAILS )); then
  log "installeren (inclusief guardrails)"
  "${here}/install.sh"
else
  log "installeren (guardrails overgeslagen)"
  "${here}/install.sh" --skip-guardrails
fi
