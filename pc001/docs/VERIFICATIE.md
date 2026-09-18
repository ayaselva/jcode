# Verificatie: Exa, de kiesbare modellen en de Hugging Face/Cerebras-provider in jcode

Deze uitvoer is letterlijk overgenomen uit de runs op PC001 (2026-09-17 voor de
Exa- en Hugging Face/Cerebras-koppeling, 2026-09-18 voor de model-picker-scope).
De gepubliceerde build staat in `~/.jcode/builds/current/jcode`; het label is de
commit-hash van de branch waaronder hij is gepubliceerd, terwijl de binary zelf
zijn broncommit meldt (`jcode --version`).

## Commando's

```bash
cd /data/worktrees/jcode/model-picker
# Let op: bouw in een worktree met een *eigen* CARGO_TARGET_DIR. Een gedeelde
# target-dir met de primaire checkout gaf een build die een rlib van de andere
# boom hergebruikte; de eerste build faalde daarmee op een import die alleen in
# deze branch bestaat.
CARGO_TARGET_DIR=/data/worktrees/jcode/model-picker/target pc001/install.sh --skip-guardrails
pc001/verify.sh
```

`install.sh` publiceert de binary naar
`~/.jcode/builds/versions/<hash>/jcode`, zet `~/.jcode/builds/current/jcode`
daarop, installeert de Doppler-launcher op `~/.local/bin/jcode`, zet
`[websearch] engine = "exa"`, alle profielen uit `pc001/providers/` en beide
pickerlijsten in `~/.jcode/config.toml` (met een getimestampte backup) en roept
daarna `verify.sh` aan. Die start vier tijdelijke servers (eigen runtime dir,
dus de server van de gebruiker blijft ongemoeid): twee voor de echte
`websearch`-tool via de debug-socket en twee voor een echte modelaanroep via
`jcode run` op de eigen socket, en vergelijkt daarna `jcode model list` met
`pc001/model-picker-models.txt`.

```
$ CARGO_TARGET_DIR=/data/worktrees/jcode/model-picker/target pc001/install.sh --skip-guardrails
▸ bouwen: scripts/dev_cargo.sh build --profile selfdev -p jcode --bin jcode
▸ publiceren naar /home/maarten/.jcode/builds/versions/<hash>/jcode
▸ current -> versions/<hash>/jcode
▸ launcher installeren: /home/maarten/.local/bin/jcode (wrapper met Doppler-sleutel)
▸ config: engine, providerprofielen en pickerlijsten in /home/maarten/.jcode/config.toml (backup config.toml.bak-model-picker-20260918T001205Z)
▸ Cerebras-catalogus opwarmen
▸ geïnstalleerd: /home/maarten/.jcode/builds/versions/<hash>/jcode
versie: <hash>   # label = de commit waaronder gepubliceerd (zie ~/.jcode/builds/current-version)
engine: exa
profielen: cheaperinference.toml databricks.toml huggingface-cerebras.toml modal-rent-b200.toml openrouter-curated.toml
pickerproviders: openai cerebras openrouter-curated cheaperinference huggingface-cerebras modal-rent-b200 databricks
pickermodellen: 24
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

De server draait ook hier via de launcher, met `HUGGINGFACE_CEREBRAS_API_KEY` en
`EXA_API_KEY` bewust ongezet. `jcode run` stuurt één bericht naar die server en
eindigt; de tokenrapportage bewijst dat de aanroep echt is gedaan.

Tot 2026-09-18 heette deze variabele `CEREBRAS_API_KEY`; die naam is nu van de
native Cerebras-provider (`api.cerebras.ai`), terwijl de Hugging Face-router zijn
eigen `HUGGINGFACE_CEREBRAS_API_KEY` kreeg. Zonder die splitsing stuurde de
launcher de Hugging Face-token naar de native provider.

```
== fase 3: modelaanroep via de Hugging Face-router op Cerebras ==
$ /home/maarten/.local/bin/jcode serve --socket /tmp/jcode-exa-verify.ic7kf0/run3/jcode.sock
$ /home/maarten/.local/bin/jcode --socket /tmp/jcode-exa-verify.ic7kf0/run3/jcode.sock run --provider-profile huggingface-cerebras -m Qwen/Qwen3.8-27B:cerebras "Antwoord uitsluitend met het woord OK."


OK
[Tokens] upload: 66970 download: 105 cache_read: 66560 cache_write: 0
OK: modelaanroep via huggingface-cerebras/Qwen/Qwen3.8-27B:cerebras
```

Deze route werkt alleen als `HUGGINGFACE_CEREBRAS_API_KEY` in het procesmilieu
van de server staat; de launcher haalt hem daar via
`huggingface-cerebras-api-key` uit Doppler `infra/all`.

## Fase 4 — ontbrekende HUGGINGFACE_CEREBRAS_API_KEY geeft een expliciete foutmelding

```
== fase 4: zonder HUGGINGFACE_CEREBRAS_API_KEY hoort jcode duidelijk te klagen ==
$ env -u HUGGINGFACE_CEREBRAS_API_KEY /home/maarten/.jcode/builds/current/jcode serve --socket /tmp/jcode-exa-verify.ic7kf0/run4/jcode.sock
$ env -u HUGGINGFACE_CEREBRAS_API_KEY /home/maarten/.jcode/builds/current/jcode --socket /tmp/jcode-exa-verify.ic7kf0/run4/jcode.sock run --provider-profile huggingface-cerebras -m Qwen/Qwen3.8-27B:cerebras "..."
Error: HUGGINGFACE_CEREBRAS_API_KEY not found in environment
```

## Fase 5 — kiesbare modellen in de picker

`verify.sh` vergelijkt `jcode model list` met `pc001/model-picker-models.txt`.
De 24 namen zijn exact de `enabledModels` van omp:

```
== fase 5: kiesbare modellen zijn exact de lijst van omp ==
Qwen/Qwen3.8-27B:cerebras
Rent-Model deepseek-ai/DeepSeek-V4.1-Flash
anthropic/claude-fable-5.1
anthropic/claude-opus-5
anthropic/claude-sonnet-5
databricks-deepseek-v4-1-flash
deepseek-v4.1-flash
deepseek/deepseek-v4-flash
deepseek/deepseek-v4.1-flash
google/gemini-3.6-flash
google/gemini-3.8-flash
gpt-5.6-sol
gpt-6-astra
gpt-oss-120b
meta/muse-spark-1.3
moonshotai/kimi-k3
openai/gpt-5.6-luna
openai/gpt-oss-120b
openai/gpt-oss-120b:cerebras
openai/gpt-oss-safeguard-20b
qwen-3.8-27b
qwen/qwen3.8-flash
z-ai/glm-5.3
z-ai/glm-5.3-flash
OK: 24 kiesbare modellen, exact de lijst van omp
```

## De live TUI-picker

`jcode model list` bewijst het CLI-pad; de picker zelf is gecontroleerd op de
echte TUI. Een server met eigen runtime dir plus een TUI-client eraan, daarna
`/model` en de pickerstatus via de debug-socket:

```bash
JCODE_RUNTIME_DIR=/tmp/jcode-picker-check JCODE_DEBUG_SOCKET=true \
  ~/.local/bin/jcode serve --socket /tmp/jcode-picker-check/jcode.sock
JCODE_RUNTIME_DIR=/tmp/jcode-picker-check ~/.local/bin/jcode --no-update \
  --socket /tmp/jcode-picker-check/jcode.sock        # TUI, daarin /model
jcode debug -s /tmp/jcode-picker-check/jcode.sock client:model-picker 2000
```

```
open True filtered_count 24 rows 24
verwacht 24 gekregen 24
ontbreekt: []
extra: []
```

De pickerrrijen komen binnen als `api_method: "remote-catalog"` met het profiel
van het actieve model als providerlabel: in deze remote modus maakt het
providerdeel van een allowlist-regel dus geen onderscheid. Daarom staat
`model-picker-models.txt` op kale modelnamen (die zijn onderling uniek); met
provider-scoped regels zakte de picker hier naar de 15 openrouter-modellen.
Ter controle: met een lege `model_picker_models` toonde dezelfde picker 423
modellen, en met de kale lijst exact de 24 hierboven.

✅ jcode gebruikt Exa, de Hugging Face/Cerebras-provider werkt met de sleutel uit
Doppler, zonder sleutel volgt een expliciete foutmelding en de picker — CLI én
TUI — toont exact de modellen van omp.

## Code-verificatie

```bash
# unit-tests van de tool (inclusief de Exa-tests, zonder netwerk)
CARGO_TARGET_DIR=/data/worktrees/jcode/model-picker/target scripts/dev_cargo.sh test --profile selfdev \
  -p jcode-app-core --lib tool::websearch
#   -> test result: ok. 21 passed; 0 failed

# config-tests (sjabloon, env-overrides, allowlist)
CARGO_TARGET_DIR=/data/worktrees/jcode/model-picker/target scripts/dev_cargo.sh test --profile selfdev \
  -p jcode-base --lib config::
#   -> test result: ok. 77 passed; 0 failed

# tests van de model-allowlist zelf
CARGO_TARGET_DIR=/data/worktrees/jcode/model-picker/target scripts/dev_cargo.sh test --profile selfdev \
  -p jcode-provider-core model_allowlist
#   -> test result: ok. 2 passed; 0 failed

# picker-tests van de TUI: dezelfde 8 falen ook op de onaangeraakte basis
# (bestaande pc001-sorteerpatch in de picker), mijn filter voegt er geen toe
CARGO_TARGET_DIR=/data/worktrees/jcode/model-picker/target scripts/dev_cargo.sh test --profile selfdev \
  -p jcode-tui model_picker
#   -> test result: FAILED. 55 passed; 8 failed   == identiek op /data/worktrees/jcode/base-check (development)

# clippy op de crates van deze wijziging
CARGO_TARGET_DIR=/data/worktrees/jcode/model-picker/target scripts/dev_cargo.sh clippy --profile selfdev \
  -p jcode-provider-core -p jcode-config-types -- -D warnings
#   -> Finished, geen waarschuwingen

# formattering van de geraakte bestanden
rustfmt --edition 2024 --check crates/jcode-provider-core/src/lib.rs \
  crates/jcode-config-types/src/lib.rs crates/jcode-base/src/provider/mod.rs \
  crates/jcode-base/src/config/default_file.rs \
  crates/jcode-tui/src/tui/app/inline_interactive.rs src/cli/commands.rs
#   -> geen diff
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

Bij de run van 2026-09-18 kwamen daar twee clippy-vondsten bij, ook buiten deze
branch: `collapsible_match` in `crates/jcode-tui-account-picker/src/overlay.rs:352`
en in `crates/jcode-render-core/src/markdown.rs`. Daarom is hier met
`--skip-guardrails` gebouwd, volgens de escape hatch die dit bestand al
beschreef; fmt, clippy op de gewijzigde crates en de gerichte tests zijn los
gedraaid (zie hierboven).
