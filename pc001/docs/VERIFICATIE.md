# Verificatie: Exa en de Hugging Face/Cerebras-provider in jcode

Deze uitvoer is letterlijk overgenomen uit de runs op PC001 (2026-09-17).
De gepubliceerde build is `~/.jcode/builds/versions/81e1c6424/jcode`: dat label
is de commit van branch `pc001-exa-websearch` waaronder hij is gepubliceerd,
terwijl de binary zelf zijn broncommit meldt (`b62a704c6`). De Rust-broncode is
sinds de Exa-branch (`2c383fffc`) niet gewijzigd — sindsdien komen er alleen
pc001-hulpmiddelen bij.

## Commando's

```bash
cd /data/worktrees/jcode/exa-websearch
CARGO_TARGET_DIR=/data/projects/jcode/target pc001/install.sh --no-build --skip-guardrails
pc001/verify.sh
```

`install.sh` publiceert de binary naar
`~/.jcode/builds/versions/<hash>/jcode`, zet `~/.jcode/builds/current/jcode`
daarop, installeert de Doppler-launcher op `~/.local/bin/jcode`, zet
`[websearch] engine = "exa"` én het providerprofiel
`[providers.huggingface-cerebras]` in `~/.jcode/config.toml` (met een
getimestampte backup) en roept daarna `verify.sh` aan. Die start vier tijdelijke
servers (eigen runtime dir, dus de server van de gebruiker blijft ongemoeid):
twee voor de echte `websearch`-tool via de debug-socket en twee voor een echte
modelaanroep via `jcode run` op de eigen socket.

```
$ CARGO_TARGET_DIR=/data/projects/jcode/target pc001/install.sh --no-build --skip-guardrails
▸ publiceren naar /home/maarten/.jcode/builds/versions/81e1c6424/jcode
▸ current -> versions/81e1c6424/jcode
▸ launcher installeren: /home/maarten/.local/bin/jcode (wrapper met Doppler-sleutel)
▸ config: engine = exa + provider huggingface-cerebras in /home/maarten/.jcode/config.toml (backup config.toml.bak-exa-20260916T224750Z)
▸ geïnstalleerd: /home/maarten/.jcode/builds/versions/81e1c6424/jcode
versie: 81e1c6424
engine: exa
provider: openai-compatible:huggingface-cerebras (default_model Qwen/Qwen3.8-27B:cerebras)
```

## Wat de binary is

```
$ env -u EXA_API_KEY jcode --version
jcode v0.64.170-dev (b62a704c6)      # broncommit van de binary

$ readlink ~/.jcode/builds/current/jcode
/home/maarten/.jcode/builds/versions/81e1c6424/jcode

$ ls -la ~/.local/bin/jcode
-rwxr-xr-x 1 maarten maarten 1891 Sep 17 00:47 /home/maarten/.local/bin/jcode     # wrapper, geen symlink
```

## Fase 0 — configuratie en sleutelbronnen

```
== jcode Exa-verificatie ==
launcher   : /home/maarten/.local/bin/jcode
binary     : /home/maarten/.jcode/builds/current/jcode (/home/maarten/.jcode/builds/versions/81e1c6424/jcode)
config     : engine = exa
provider   : openai-compatible:huggingface-cerebras (default_model Qwen/Qwen3.8-27B:cerebras)
sleutelbron: exa-cli/Doppler
sleutelbron: huggingface-cerebras-api-key/Doppler
```

## Fase 1 — echte zoekopdracht, sleutel runtime uit Doppler

`verify.sh` start de server via de launcher (`/home/maarten/.local/bin/jcode`)
met `env -u EXA_API_KEY`, zodat de sleutel alleen via `exa-cli print-key`
→ Doppler `infra/all` binnen kan komen. Letterlijke uitvoer:

```
== fase 1: echte zoekopdracht (sleutel runtime uit Doppler) ==
$ /home/maarten/.local/bin/jcode serve --socket /tmp/jcode-exa-verify.qyQUwj/run1/jcode.sock
sessie: session_daisy_1789598872701_381a7c0c4f5932b4
$ /home/maarten/.local/bin/jcode debug -s /tmp/jcode-exa-verify.qyQUwj/run1/jcode.sock 'tool:websearch {"query":"Exa semantic search API"}'
Search results for: Exa semantic search API

1. **https://exa.ai/docs/reference/search**
   https://exa.ai/docs/reference/search
   > The search endpoint lets you search the web and extract contents from the results.
[... resultaat 1, 2 en 3 hier afgekapt: elke snippet is een echte
Exa-highlight van ±1000 tekens paginatekst (zie de cap van 1200 tekens in
parse_exa_results) ...]

provider: exa (requestId 3fdf4d9c3eaef078e77543ccc8368c23)

OK: provider = exa
```

De regel `provider: exa (requestId ...)` is het bewijs: de Exa-`requestId` komt
uit het antwoord van `POST https://api.exa.ai/search`, dus de zoekopdracht is
echt door Exa uitgevoerd. Zonder `EXA_API_KEY` in de omgeving kan die regel niet
verschijnen: de launcher haalt de sleutel dan uit Doppler.

## Fase 2 — ontbrekende Exa-sleutel geeft een expliciete foutmelding

```
== fase 2: zonder sleutel hoort jcode duidelijk te klagen ==
$ env -u EXA_API_KEY /home/maarten/.jcode/builds/current/jcode serve --socket /tmp/jcode-exa-verify.qyQUwj/run2/jcode.sock
$ env -u EXA_API_KEY /home/maarten/.jcode/builds/current/jcode debug -s /tmp/jcode-exa-verify.qyQUwj/run2/jcode.sock 'tool:websearch {"query":"..."}'
Error: Exa engine selected but no API key is available. Set `websearch.exa_api_key` in your config, or export the EXA_API_KEY environment variable (the PC001 launcher fetches it from the Doppler `infra/all` secret).
```

De server is hier direct uit de binary gestart (buiten de launcher om), dus er
is geen enkele weg waarlangs een sleutel binnenkomt; de tool faalt dan met de
boodschap hierboven in plaats van stil over te stappen op een fallback-engine.

## Fase 3 — echte modelaanroep via Hugging Face/Cerebras

De server draait ook hier via de launcher, met `CEREBRAS_API_KEY` en
`EXA_API_KEY` bewust ongezet. `jcode run` stuurt één bericht naar die server en
eindigt; de tokenrapportage bewijst dat de aanroep echt is gedaan.

```
== fase 3: modelaanroep via de Hugging Face-router op Cerebras ==
$ /home/maarten/.local/bin/jcode serve --socket /tmp/jcode-exa-verify.qyQUwj/run3/jcode.sock
$ /home/maarten/.local/bin/jcode --socket /tmp/jcode-exa-verify.qyQUwj/run3/jcode.sock run --provider-profile huggingface-cerebras -m Qwen/Qwen3.8-27B:cerebras "Antwoord uitsluitend met het woord OK."


OK
[Tokens] upload: 66583 download: 265 cache_read: 65536 cache_write: 0
OK: modelaanroep via huggingface-cerebras/Qwen/Qwen3.8-27B:cerebras
```

Deze route werkt alleen als `CEREBRAS_API_KEY` in het procesmilieu van de server
staat; de launcher haalt hem daar via `huggingface-cerebras-api-key` uit Doppler
`infra/all`.

## Fase 4 — ontbrekende CEREBRAS_API_KEY geeft een expliciete foutmelding

```
== fase 4: zonder CEREBRAS_API_KEY hoort jcode duidelijk te klagen ==
$ env -u CEREBRAS_API_KEY /home/maarten/.jcode/builds/current/jcode serve --socket /tmp/jcode-exa-verify.qyQUwj/run4/jcode.sock
$ env -u CEREBRAS_API_KEY /home/maarten/.jcode/builds/current/jcode --socket /tmp/jcode-exa-verify.qyQUwj/run4/jcode.sock run --provider-profile huggingface-cerebras -m Qwen/Qwen3.8-27B:cerebras "..."
Error: CEREBRAS_API_KEY not found in environment

✅ jcode gebruikt Exa; de Hugging Face/Cerebras-provider werkt met de sleutel uit Doppler en zonder sleutel volgt een expliciete foutmelding.
```

## Welke modellen de picker ziet

```
$ jcode model list | grep -E 'gpt-oss-120b:cerebras|Qwen3.8-27B:cerebras'
openai/gpt-oss-120b:cerebras
Qwen/Qwen3.8-27B:cerebras
```

Beide ID's komen uit `pc001/providers/huggingface-cerebras.toml`; de ingang
`openai-compatible:huggingface-cerebras` staat in `model_picker_providers`.

## Code-verificatie

```bash
# unit-tests van de tool (inclusief de nieuwe Exa-tests, zonder netwerk)
CARGO_TARGET_DIR=/data/projects/jcode/target scripts/dev_cargo.sh test --profile selfdev \
  -p jcode-app-core --lib tool::websearch
#   -> test result: ok. 21 passed; 0 failed

# config-tests (sjabloon, env-overrides, allowlist)
CARGO_TARGET_DIR=/data/projects/jcode/target scripts/dev_cargo.sh test --profile selfdev \
  -p jcode-base --lib config::
#   -> test result: ok. 77 passed; 0 failed

# formattering van de geraakte bestanden
rustfmt --edition 2024 --check crates/jcode-app-core/src/tool/websearch.rs \
  crates/jcode-base/src/config.rs crates/jcode-base/src/config/default_file.rs \
  crates/jcode-base/src/config/env_overrides.rs crates/jcode-base/src/message.rs \
  crates/jcode-config-types/src/lib.rs crates/jcode-config-types/src/websearch.rs
#   -> geen diff

# ratchets: geen van de geraakte bestanden komt in de rapporten voor
python3 scripts/check_code_size_budget.py | grep -c websearch      # 0
python3 scripts/check_swallowed_error_budget.py | grep -c websearch # 0
```

`scripts/check_guardrails.sh` is rood op deze checkout, maar uitsluitend op
gates die al rood waren vóór deze branch: `cargo fmt --all --check` struikelt
over `crates/jcode-base/src/prompt_tests.rs` en
`crates/jcode-tui/src/tui/ui_inline_interactive.rs` (beide onaangeraakt), en
clippy 1.95 meldt `collapsible_match`/`single_match` in
`crates/jcode-base/src/provider/catalog_routes.rs:1174`,
`crates/jcode-base/src/usage/accessors.rs:168`,
`crates/jcode-provider-bedrock/src/lib.rs:1283` en
`crates/jcode-render-core/src/markdown.rs:423`. Dezelfde run in de onaangeraakte
checkout `/data/projects/jcode` geeft exact dezelfde vondsten, op geen enkele
regel uit deze branch.
