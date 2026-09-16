# PC001 Exa-koppeling voor jcode

Exa (`https://api.exa.ai/search`) is de standaard websearch-provider van jcode op
PC001. Deze map bevat alles wat daarvoor nodig is; de rest van de wijziging zit
in de jcode-broncode zelf:

| Onderdeel | Bestand |
|---|---|
| `exa`-engine (POST `/search`, `x-api-key`, highlights) | `crates/jcode-app-core/src/tool/websearch.rs` |
| `WebSearchEngine::Exa` + `WebSearchConfig.exa_api_key(_env)` | `crates/jcode-config-types/src/lib.rs` |
| env-overrides `JCODE_EXA_API_KEY`, `JCODE_EXA_API_KEY_ENV` | `crates/jcode-base/src/config/env_overrides.rs` |
| allowlist `CONFIG_ENV_KEYS` | `crates/jcode-base/src/config.rs` |
| standaard `engine = "exa"` in nieuwe configs | `crates/jcode-base/src/config/default_file.rs` |
| `EXA_API_KEY` in de secret-redactie | `crates/jcode-base/src/message.rs` |

## Sleutelbeleid

De Exa-sleutel staat **nooit** in een bestand, commit, log of op de
opdrachtregel. De launcher (`pc001/jcode-launcher`, geïnstalleerd als
`~/.local/bin/jcode`) haalt hem bij elke start op via de gedeelde helper
`exa-cli`:

```
exa-cli print-key   # omgeving EXA_API_KEY, anders Doppler infra/all (15 min cache)
```

Daarna `exec`t de wrapper de echte binary
(`~/.jcode/builds/current/jcode`) met `EXA_API_KEY` alleen in het
procesmilieu. De binary leest die variabele (of `websearch.exa_api_key` in
`~/.jcode/config.toml`); de sleutelnaam is configureerbaar met
`JCODE_EXA_API_KEY_ENV` / `websearch.exa_api_key_env`.

## Scripts

```bash
# installeren/updaten: bouwen, publiceren, wrapper + config zetten, verifiëren
pc001/install.sh
pc001/install.sh --no-build          # alleen publiceren/wrapper/config
pc001/install.sh --skip-guardrails   # zonder fmt/clippy/ratchets

# upstream ophalen, branch rebasen, daarna install.sh
pc001/upgrade.sh
pc001/upgrade.sh --skip-guardrails   # als check_guardrails.sh al rood staat buiten deze branch

# bewijs met echte zoekopdrachten dat de geïnstalleerde jcode Exa gebruikt
pc001/verify.sh
```

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

* `pc001-exa-websearch` — deze integratie.
* `myplace` = `ssh://git@git.myplaceonline.nl:2222/myplace/jcode.git`.
* `fork` = `https://github.com/ayaselva/jcode.git` (alleen als je rechten hebt).

## Verificatie

Zie `docs/VERIFICATIE.md` voor de letterlijke commando's en de waargenomen
uitvoer.
