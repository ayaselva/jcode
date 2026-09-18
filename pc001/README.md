# PC001-koppeling voor jcode

Drie onderdelen van jcode op PC001:

* **Exa** (`https://api.exa.ai/search`) is de standaard websearch-provider; de
  engine-wijziging daarvoor zit in de jcode-broncode zelf.
* **De kiesbare modellen** zijn exact de `enabledModels` van omp. Dat is een
  combinatie van broncode (een allowlist `provider.model_picker_models`) en
  configuratie: vijf providerprofielen in `pc001/providers/` plus de twee
  pickerlijsten in deze map.
* **Hugging Face/Cerebras** (`https://router.huggingface.co/v1`, de modellen
  `openai/gpt-oss-120b:cerebras` en `Qwen/Qwen3.8-27B:cerebras`) is één van die
  vijf profielen.

Deze map bevat alles wat daarvoor nodig is; de rest van de wijziging zit
in de jcode-broncode zelf:

| Onderdeel | Bestand |
|---|---|
| `exa`-engine (POST `/search`, `x-api-key`, highlights) | `crates/jcode-app-core/src/tool/websearch.rs` |
| `WebSearchEngine::Exa` + `WebSearchConfig.exa_api_key(_env)` | `crates/jcode-config-types/src/lib.rs` |
| allowlist `ProviderConfig.model_picker_models` | `crates/jcode-config-types/src/lib.rs` |
| filter `filter_model_routes_by_model_allowlist` | `crates/jcode-provider-core/src/lib.rs` |
| toepassing in de picker | `crates/jcode-tui/src/tui/app/inline_interactive.rs` |
| toepassing in `jcode model list` | `src/cli/commands.rs` |
| documentatie in nieuwe configs | `crates/jcode-base/src/config/default_file.rs` |
| env-overrides `JCODE_EXA_API_KEY`, `JCODE_EXA_API_KEY_ENV` | `crates/jcode-base/src/config/env_overrides.rs` |
| allowlist `CONFIG_ENV_KEYS` | `crates/jcode-base/src/config.rs` |
| standaard `engine = "exa"` in nieuwe configs | `crates/jcode-base/src/config/default_file.rs` |
| `EXA_API_KEY` in de secret-redactie | `crates/jcode-base/src/message.rs` |

## Kiesbare modellen

* `[provider] model_picker_models` begrenst `/model` én `jcode model list` tot
  de opgegeven modellen. Een regel is een modelnaam precies zoals de picker die
  toont (`gpt-oss-120b`, `openai/gpt-oss-120b`, `Qwen/Qwen3.8-27B:cerebras`); de
  hele naam wordt hoofdletterongevoelig vergeleken, dus een naam met een `/` of
  `:` erin wordt nooit als providerprefix gelezen. Providers begrens je met
  `model_picker_providers`. Routes van het actieve model blijven altijd
  zichtbaar en een lijst zonder enkele match valt terug op de ongefilterde
  routes, zodat de picker nooit leeg wordt.
* `pc001/model-picker-models.txt` is de bron van waarheid voor die lijst: de 24
  modellen van omp (`enabledModels`), als modelnaam. Provider-scoped regels
  bestaan bewust niet: de normale TUI praat met een remote server en die remote
  catalogus labelt elke route met het profiel van het actieve model
  (`remote-catalog`), en namen als `openai/gpt-oss-120b` zijn zelf modelnamen in
  plaats van provider/model-paren. `pc001/model-picker-providers.txt` zet
  `[provider] model_picker_providers` op precies de providers van die modellen,
  zodat de providerfilter in een lokale picker geen routes wegneemt die er juist
  wel in horen.
* De modellen komen per provider uit een profiel in `pc001/providers/*.toml`:
  `openrouter-curated` (15 modellen), `cheaperinference` (1),
  `huggingface-cerebras` (2), `modal-rent-b200` (1) en `databricks` (1). Elk
  bestand is de bron van waarheid voor dat `[providers.<naam>]`-blok;
  `config-add-provider.py --set` vervangt het bestaande blok, zodat de
  modellijst in de config niet kan afwijken van het bestand.
* `cerebras` (2 modellen) en `openai` (2 modellen) zijn ingebouwde providers van
  jcode zelf; die hebben geen profielbestand nodig. Cerebras heeft in jcode geen
  statische modellijst: de picker leest de live cataloguscache. `install.sh`
  warmt die cache daarom met `jcode model list -p cerebras`, waarna de picker
  hetzelfde tweetal toont als de Cerebras-API (en dus als omp).

## Sleutelbeleid

Alle sleutels staan **nooit** in een bestand, commit, log of op de
opdrachtregel. De launcher (`pc001/jcode-launcher`, geïnstalleerd als
`~/.local/bin/jcode`) haalt ze bij elke start op via de gedeelde helpers en
`exec`t daarna de echte binary (`~/.jcode/builds/current/jcode`) met alleen de
sleutels in het procesmilieu:

| Variabele | Helper |
|---|---|
| `EXA_API_KEY` | `exa-cli print-key` — omgeving, anders Doppler `infra/all` (15 min cache) |
| `CEREBRAS_API_KEY` | `cerebras-api-key` — Doppler-secret `CEREBRAS_API_KEY`, native Cerebras (`api.cerebras.ai`), vormcontrole `csk-*` |
| `HUGGINGFACE_CEREBRAS_API_KEY` | `huggingface-cerebras-api-key` — Doppler-secret `HF_TOKEN`, Hugging Face-router, vormcontrole `hf_*` |
| `MODAL_RENT_API_KEY` | `modal-rent-api-key` — gehuurde Modal-B200-server (`rent-server`) |
| `DATABRICKS_TRIAL_TOKEN` | `databricks-api-key` — Doppler-secret `DATABRICKS_TRIAL_TOKEN`, Databricks Foundation Model APIs, vormcontrole `dapi*` |

De twee Cerebras-sleutels staan bewust apart: de native provider
(`api.cerebras.ai`, `api_key_env = CEREBRAS_API_KEY`) en de Hugging Face-router
(`router.huggingface.co`, `api_key_env = HUGGINGFACE_CEREBRAS_API_KEY`) zijn
verschillende endpoints met verschillende credentials. Eén gedeelde
`CEREBRAS_API_KEY` stuurde de Hugging Face-token naar de native provider.

De helpers komen uit `/data/projects/omp-agent/` (zie `setup.sh` daar); hun
Doppler-secretnamen zijn op 18-09-2026 bijgesteld naar de herordening
(`HF_TOKEN` voor de Hugging Face-token, `CEREBRAS_API_KEY` voor de native
Cerebras-sleutel; daarvoor `CEREBRAS_API_KEY` respectievelijk `CEREBRAS`).

De binary leest deze variabelen (of een opgeslagen sleutel in
`~/.jcode/<profiel>.env`). Omdat de omgeving vóór het env-bestand gaat, wint de
runtime-helper altijd van een achtergebleven oude sleutel in dat bestand.

Eén inhoudelijk verschil met omp blijft: omp laat `cerebras/*` bij een 402-quota
automatisch terugvallen op de Hugging Face-tweeling (`retry.fallbackChains`);
jcode kent zo'n keten niet, dus kies daar dan zelf `huggingface-cerebras/…`. Het
account geeft op beide native modellen `402 payment_required_error param=quota`.

## Scripts

```bash
# installeren/updaten: bouwen, publiceren, wrapper + profielen + pickerlijsten, verifiëren
pc001/install.sh
pc001/install.sh --no-build          # alleen publiceren/wrapper/config
pc001/install.sh --skip-guardrails   # zonder fmt/clippy/ratchets

# upstream ophalen, branch rebasen, daarna install.sh
pc001/upgrade.sh
pc001/upgrade.sh --skip-guardrails   # als check_guardrails.sh al rood staat buiten deze branch

# bewijs met echte zoekopdrachten dat de geïnstalleerde jcode Exa gebruikt,
# met een echte modelaanroep dat de Hugging Face/Cerebras-route werkt en met
# `jcode model list` dat de picker exact de modellen van omp toont
pc001/verify.sh
```

`config-set-engine.py` zet `[websearch] engine`; `config-add-provider.py` voegt
een providerprofiel toe of bij (`--set`) en zet de picker-ingang;
`config-set-model-picker.py` schrijft `model_picker_providers` en
`model_picker_models` uit de twee lijstbestanden. Alle drie zijn idempotent en
laten de rest van `~/.jcode/config.toml` ongemoeid, met een tijdgestempelde
back-up vooraf.

`install.sh` publiceert naar `~/.jcode/builds/versions/<git-hash>/jcode` en zet
de `current`-symlink daarop; `~/.jcode/builds/stable` blijft ongemoeid.
jcode's eigen updater vervangt de launcher-wrapper weer door een symlink — draai
daarna `pc001/install.sh` (of `pc001/upgrade.sh`) opnieuw.

## Upgraden naar een nieuwe upstream

`upgrade.sh` voegt zo nodig de remote `upstream`
(`https://github.com/1jehuang/jcode.git`) toe, doet `git fetch --all --prune`,
rebase't de huidige branch op `upstream/main` (of `master`) als die nog niet in
onze geschiedenis zit — en breekt bij conflicten netjes af met de handmatige
commando's — en roept daarna `install.sh` aan (guardrails, build, publicatie,
launcher, config, verificatie).

Upstream wordt niet gevendord: de fork bevat alleen onze eigen commits.

## Branches

* `pc001-exa-websearch` — de Exa-integratie.
* `pc001-model-picker` — de model-picker-scope (de `enabledModels` van omp).
* `myplace` = `ssh://git@git.myplaceonline.nl:2222/myplace/jcode.git`.
* `fork` = `https://github.com/ayaselva/jcode.git` (alleen als je rechten hebt).

## Verificatie

Zie `docs/VERIFICATIE.md` voor de letterlijke commando's en de waargenomen
uitvoer.
