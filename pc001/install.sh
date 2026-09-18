#!/usr/bin/env bash
# Installeer de PC001-koppeling voor jcode op deze machine. Idempotent:
# bouwen, publiceren naar ~/.jcode/builds/current, de Doppler-launcher
# installeren, [websearch] engine = "exa" zetten, de providerprofielen uit
# pc001/providers/ in de config zetten en de picker begrenzen op de modellen
# uit pc001/model-picker-models.txt (de `enabledModels` van omp).
#
# Gebruik:
#   pc001/install.sh                 # bouwen + publiceren + wrapper + config + verify
#   pc001/install.sh --no-build      # bestaande target/selfdev/jcode publiceren
#   pc001/install.sh --no-verify     # sla de live zoekopdracht over
#   pc001/install.sh --skip-guardrails
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "${here}/.." && pwd)
jcode_home="${JCODE_HOME:-${HOME}/.jcode}"
builds_dir="${jcode_home}/builds"
bin_dir="${HOME}/.local/bin"

BUILD=1
VERIFY=1
GUARDRAILS=1
for arg in "$@"; do
  case "$arg" in
    --no-build) BUILD=0 ;;
    --no-verify) VERIFY=0 ;;
    --skip-guardrails) GUARDRAILS=0 ;;
    -h|--help)
      sed -n '2,11p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "install.sh: onbekende optie '$arg'" >&2
      exit 2
      ;;
  esac
done

log() { printf '▸ %s\n' "$*"; }

if [[ -n "$(git -C "${repo}" status --porcelain)" ]]; then
  echo "install.sh: werkboom ${repo} is niet schoon; commit eerst (de versielabel wordt de git-hash)" >&2
  git -C "${repo}" status --short >&2
  exit 1
fi

label=$(git -C "${repo}" rev-parse --short HEAD)

if (( GUARDRAILS )); then
  log "guardrails (fmt + clippy + ratchets)"
  "${repo}/scripts/check_guardrails.sh"
fi

if (( BUILD )); then
  log "bouwen: scripts/dev_cargo.sh build --profile selfdev -p jcode --bin jcode"
  (cd "${repo}" && scripts/dev_cargo.sh build --profile selfdev -p jcode --bin jcode)
fi

binary="${CARGO_TARGET_DIR:-${repo}/target}/selfdev/jcode"
if [[ ! -x "${binary}" ]]; then
  echo "install.sh: geen binary op ${binary}" >&2
  exit 1
fi

log "publiceren naar ${builds_dir}/versions/${label}/jcode"
mkdir -p "${builds_dir}/versions/${label}"
dest="${builds_dir}/versions/${label}/jcode"
rm -f "${dest}"
ln "${binary}" "${dest}" 2>/dev/null || cp -f "${binary}" "${dest}"
chmod 0755 "${dest}"

log "current -> versions/${label}/jcode"
ln -sfn "${dest}" "${builds_dir}/current/jcode.new"
mv -fT "${builds_dir}/current/jcode.new" "${builds_dir}/current/jcode"
printf '%s\n' "${label}" > "${builds_dir}/current-version"

log "launcher installeren: ${bin_dir}/jcode (wrapper met Doppler-sleutel)"
install -d "${bin_dir}"
install -m 0755 "${here}/jcode-launcher" "${bin_dir}/jcode"

config="${jcode_home}/config.toml"

if [[ -f "${config}" ]]; then
  backup="${config}.bak-model-picker-$(date -u +%Y%m%dT%H%M%SZ)"
  cp -p "${config}" "${backup}"
  log "config: engine, providerprofielen en pickerlijsten in ${config} (backup ${backup##*/})"
else
  log "config: ${config} aanmaken met [websearch] engine = exa en de providerprofielen"
fi
python3 "${here}/config-set-engine.py" "${config}" exa '"bing"'
# Elk profielbestand in pc001/providers/ is de bron van waarheid voor dat blok:
# --set vervangt een bestaand blok (inclusief zijn [[...models]]-subtables),
# zodat de modellijst in de config altijd gelijk is aan het bestand.
for block in "${here}"/providers/*.toml; do
  profile="$(basename "${block}" .toml)"
  python3 "${here}/config-add-provider.py" --set "${config}" "${block}" "${profile}"
done
python3 "${here}/config-set-model-picker.py" \
  "${config}" "${here}/model-picker-providers.txt" "${here}/model-picker-models.txt"

# De native Cerebras-provider is een ingebouwde jcode-profiel zonder statische
# modellen: zonder cataloguscache toont de picker alleen zijn default_model.
# Deze aanroep haalt /v1/models op en legt die cache aan, waarna de picker
# hetzelfde tweetal toont als omp.
if [[ -x "${bin_dir}/jcode" ]]; then
  log "Cerebras-catalogus opwarmen"
  "${bin_dir}/jcode" model list -p cerebras >/dev/null
fi

log "geïnstalleerd: ${dest}"
printf 'versie: %s\n' "${label}"
printf 'engine: %s\n' "$(python3 "${here}/config-set-engine.py" --get "${config}")"
printf 'profielen: %s\n' "$(cd "${here}/providers" && printf '%s ' *.toml)"
printf 'pickerproviders: %s\n' \
  "$(python3 "${here}/config-set-model-picker.py" --get "${config}" model_picker_providers | tr '\n' ' ')"
printf 'pickermodellen: %s\n' \
  "$(python3 "${here}/config-set-model-picker.py" --get "${config}" model_picker_models | wc -l)"

if (( VERIFY )); then
  log "verificatie"
  "${here}/verify.sh"
fi
