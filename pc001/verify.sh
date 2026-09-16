#!/usr/bin/env bash
# Bewijs dat de geïnstalleerde jcode Exa als zoekprovider gebruikt en dat de
# Hugging Face/Cerebras-provider werkt.
#
# Aanpak: start een tijdelijke jcode-server (eigen runtime dir, zodat de server
# van de gebruiker ongemoeid blijft) uit de geïnstalleerde binary, maak daar één
# sessie in en laat die server de websearch-tool echt uitvoeren via de
# debug-socket. Dat is precies dezelfde tool die het model krijgt, maar zonder
# modelcall. Voor de Hugging Face-provider doet die tijdelijke server wél een
# echte modelaanroep, via `jcode run` op zijn eigen socket.
#
#   fase 1 (positief): server gestart via de launcher-wrapper ~/.local/bin/jcode
#     => EXA_API_KEY komt runtime uit Doppler (infra/all). Verwacht:
#        provider: exa (requestId ...)
#   fase 2 (negatief): server gestart met `env -u EXA_API_KEY` uit dezelfde
#     binary. Verwacht: duidelijke foutmelding dat de Exa-sleutel ontbreekt.
#   fase 3 (positief): server gestart via de launcher-wrapper, CEREBRAS_API_KEY
#     bewust ongezet => de sleutel komt runtime uit Doppler. Verwacht: een echt
#     modelantwoord van het Hugging Face/Cerebras-profiel plus tokenrapportage.
#   fase 4 (negatief): dezelfde aanroep op een server uit de kale binary zonder
#     CEREBRAS_API_KEY. Verwacht: "CEREBRAS_API_KEY not found in environment".
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
jcode_home="${JCODE_HOME:-${HOME}/.jcode}"
launcher="${JCODE_LAUNCHER:-${HOME}/.local/bin/jcode}"
real_bin="${JCODE_REAL_BIN:-${jcode_home}/builds/current/jcode}"
query="${JCODE_VERIFY_QUERY:-Exa semantic search API}"
hf_profile="${JCODE_VERIFY_PROFILE:-huggingface-cerebras}"
hf_model="${JCODE_VERIFY_MODEL:-Qwen/Qwen3.8-27B:cerebras}"
hf_prompt="${JCODE_VERIFY_PROMPT:-Antwoord uitsluitend met het woord OK.}"

fail() {
  printf 'verify: FOUT: %s\n' "$*" >&2
  exit 1
}

[[ -x "${launcher}" ]] || fail "launcher ${launcher} ontbreekt; draai pc001/install.sh"
[[ -x "${real_bin}" ]] || fail "binary ${real_bin} ontbreekt; draai pc001/install.sh"

workdir=$(mktemp -d "${TMPDIR:-/tmp}/jcode-exa-verify.XXXXXX")
servers=()

cleanup() {
  local pid
  for pid in "${servers[@]:-}"; do
    [[ -n "${pid}" ]] && kill "${pid}" 2>/dev/null || true
  done
  for pid in "${servers[@]:-}"; do
    [[ -n "${pid}" ]] && wait "${pid}" 2>/dev/null || true
  done
  rm -rf "${workdir}"
}
trap cleanup EXIT

# ---- helpers ---------------------------------------------------------------

start_server() { # $1=runtime dir, $2=server log, rest: env command prefix
  local runtime="$1" log="$2"
  shift 2
  mkdir -p "${runtime}"
  JCODE_RUNTIME_DIR="${runtime}" JCODE_DEBUG_SOCKET=true \
    setsid "$@" serve --socket "${runtime}/jcode.sock" >"${log}" 2>&1 &
  servers+=("$!")
  local waited=0
  while [[ ! -S "${runtime}/jcode-debug.sock" ]]; do
    (( waited++ >= 300 )) && fail "server startte niet binnen 30s; zie ${log}"
    sleep 0.1
  done
}

create_session() { # $1=runtime dir, $2=binary to talk with
  local runtime="$1" bin="$2"
  JCODE_RUNTIME_DIR="${runtime}" "${bin}" --no-update debug \
    -s "${runtime}/jcode.sock" create_session
}

run_websearch() { # $1=runtime dir, $2=binary, $3=json input
  local runtime="$1" bin="$2" input="$3"
  JCODE_RUNTIME_DIR="${runtime}" "${bin}" --no-update debug \
    -s "${runtime}/jcode.sock" "tool:websearch ${input}"
}

# Ruim de tijdelijke sessie op die deze verificatie zelf aanmaakte: alleen de
# bestanden van precies dit session-id, en alleen als ze bestaan.
drop_session() { # $1=session id
  local id="$1"
  [[ -n "${id}" ]] || return 0
  rm -f "${jcode_home}/sessions/${id}.json" \
        "${jcode_home}/sessions/${id}.bak" 2>/dev/null || true
  if command -v sqlite3 >/dev/null 2>&1; then
    sqlite3 "${jcode_home}/session-metadata-v1.sqlite3" \
      "DELETE FROM recent_sessions WHERE session_id='${id}';" \
      >/dev/null 2>&1 || true
  fi
}

# ---- fase 0: configuratie --------------------------------------------------

echo "== jcode Exa-verificatie =="
echo "launcher   : ${launcher}"
echo "binary     : ${real_bin} ($(readlink -f "${real_bin}"))"
echo "config     : engine = $(python3 "${here}/config-set-engine.py" --get "${jcode_home}/config.toml")"
echo "provider   : $(python3 "${here}/config-add-provider.py" --get-picker "${jcode_home}/config.toml" "openai-compatible:${hf_profile}") (default_model $(python3 "${here}/config-add-provider.py" --get "${jcode_home}/config.toml" "${hf_profile}"))"
echo "sleutelbron: $(command -v exa-cli >/dev/null 2>&1 && echo exa-cli/Doppler || echo 'exa-cli ONTBREEKT')"
echo "sleutelbron: $(command -v huggingface-cerebras-api-key >/dev/null 2>&1 && echo huggingface-cerebras-api-key/Doppler || echo 'huggingface-cerebras-api-key ONTBREEKT')"
echo

# ---- fase 1: echte zoekopdracht via Exa ------------------------------------

runtime1="${workdir}/run1"
echo "== fase 1: echte zoekopdracht (sleutel runtime uit Doppler) =="
echo "\$ ${launcher} serve --socket ${runtime1}/jcode.sock"
start_server "${runtime1}" "${workdir}/server1.log" env -u EXA_API_KEY "${launcher}"

session_json=$(create_session "${runtime1}" "${launcher}")
session_id=$(printf '%s' "${session_json}" | python3 -c 'import json,sys;print(json.load(sys.stdin)["session_id"])')
echo "sessie: ${session_id}"

echo "\$ ${launcher} debug -s ${runtime1}/jcode.sock 'tool:websearch {\"query\":\"${query}\"}'"
result=$(run_websearch "${runtime1}" "${launcher}" "{\"query\":\"${query}\",\"num_results\":3}") || fail "websearch-tool faalde: ${result}"
printf '%s\n' "${result}" | python3 -c 'import json,sys;print(json.load(sys.stdin)["output"])'

printf '%s' "${result}" | grep -q 'provider: exa' \
  || fail "de zoekopdracht gebruikte Exa niet (geen 'provider: exa' in de output)"
printf '%s' "${result}" | grep -q 'requestId' \
  || fail "geen Exa-requestId in de output"
echo "OK: provider = exa"

drop_session "${session_id}"
echo

# ---- fase 2: ontbrekende sleutel ------------------------------------------

runtime2="${workdir}/run2"
echo "== fase 2: zonder sleutel hoort jcode duidelijk te klagen =="
echo "\$ env -u EXA_API_KEY ${real_bin} serve --socket ${runtime2}/jcode.sock"
start_server "${runtime2}" "${workdir}/server2.log" env -u EXA_API_KEY "${real_bin}"

session_json2=$(create_session "${runtime2}" "${real_bin}")
session_id2=$(printf '%s' "${session_json2}" | python3 -c 'import json,sys;print(json.load(sys.stdin)["session_id"])')

echo "\$ env -u EXA_API_KEY ${real_bin} debug -s ${runtime2}/jcode.sock 'tool:websearch {\"query\":\"...\"}'"
set +e
error_out=$(JCODE_RUNTIME_DIR="${runtime2}" env -u EXA_API_KEY "${real_bin}" --no-update debug \
  -s "${runtime2}/jcode.sock" "tool:websearch {\"query\":\"${query}\"}" 2>&1)
set -e
printf '%s\n' "${error_out}"
printf '%s' "${error_out}" | grep -qi 'no API key' \
  || fail "ontbrekende sleutel gaf geen duidelijke foutmelding"

drop_session "${session_id2}"
echo

# ---- fase 3: modelaanroep via Hugging Face/Cerebras ------------------------

runtime3="${workdir}/run3"
echo "== fase 3: modelaanroep via de Hugging Face-router op Cerebras =="
echo "\$ ${launcher} serve --socket ${runtime3}/jcode.sock"
start_server "${runtime3}" "${workdir}/server3.log" env -u EXA_API_KEY -u CEREBRAS_API_KEY "${launcher}"

echo "\$ ${launcher} --socket ${runtime3}/jcode.sock run --provider-profile ${hf_profile} -m ${hf_model} \"${hf_prompt}\""
set +e
answer=$(JCODE_RUNTIME_DIR="${runtime3}" env -u EXA_API_KEY -u CEREBRAS_API_KEY "${launcher}" --no-update \
  --socket "${runtime3}/jcode.sock" run --provider-profile "${hf_profile}" -m "${hf_model}" "${hf_prompt}" 2>&1)
status=$?
set -e
printf '%s\n' "${answer}"
[[ ${status} -eq 0 ]] || fail "jcode run faalde (exit ${status})"
printf '%s' "${answer}" | grep -q 'OK' \
  || fail "geen bruikbaar antwoord van het Hugging Face/Cerebras-model"
printf '%s' "${answer}" | grep -q '\[Tokens\]' \
  || fail "geen tokenrapportage; is er echt een modelaanroep gedaan?"
if printf '%s' "${answer}" | grep -Eqi 'not found in environment|unauthorized|invalid api key'; then
  fail "de Hugging Face/Cerebras-sleutel kwam niet aan bij de provider"
fi
echo "OK: modelaanroep via ${hf_profile}/${hf_model}"

# ---- fase 4: ontbrekende Hugging Face/Cerebras-sleutel --------------------

runtime4="${workdir}/run4"
echo "== fase 4: zonder CEREBRAS_API_KEY hoort jcode duidelijk te klagen =="
echo "\$ env -u CEREBRAS_API_KEY ${real_bin} serve --socket ${runtime4}/jcode.sock"
start_server "${runtime4}" "${workdir}/server4.log" env -u EXA_API_KEY -u CEREBRAS_API_KEY "${real_bin}"

echo "\$ env -u CEREBRAS_API_KEY ${real_bin} --socket ${runtime4}/jcode.sock run --provider-profile ${hf_profile} -m ${hf_model} \"...\""
set +e
hf_error=$(JCODE_RUNTIME_DIR="${runtime4}" env -u EXA_API_KEY -u CEREBRAS_API_KEY "${real_bin}" --no-update \
  --socket "${runtime4}/jcode.sock" run --provider-profile "${hf_profile}" -m "${hf_model}" "${hf_prompt}" 2>&1)
hf_status=$?
set -e
printf '%s\n' "${hf_error}"
[[ ${hf_status} -ne 0 ]] || fail "zonder CEREBRAS_API_KEY hoort de modelaanroep te mislukken"
printf '%s' "${hf_error}" | grep -q 'CEREBRAS_API_KEY not found' \
  || fail "ontbrekende Hugging Face/Cerebras-sleutel gaf geen duidelijke foutmelding"

echo
echo "✅ jcode gebruikt Exa; de Hugging Face/Cerebras-provider werkt met de sleutel uit Doppler en zonder sleutel volgt een expliciete foutmelding."
