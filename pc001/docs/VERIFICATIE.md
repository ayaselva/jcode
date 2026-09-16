# Verificatie: jcode gebruikt Exa als zoekprovider

Deze uitvoer is letterlijk overgenomen uit de runs op PC001 (2026-09-16).
De gebruikte build is commit `2c383fffc` van branch `pc001-exa-websearch`
(`jcode v0.64.168-dev (2c383fffc)`); de gepubliceerde build is dezelfde code op
een docs-commit na.

## Commando's

```bash
cd /data/worktrees/jcode/exa-websearch
CARGO_TARGET_DIR=/data/projects/jcode/target scripts/dev_cargo.sh build --profile selfdev -p jcode --bin jcode
CARGO_TARGET_DIR=/data/projects/jcode/target pc001/install.sh --no-build --skip-guardrails
pc001/verify.sh
```

`install.sh` publiceert de binary naar
`~/.jcode/builds/versions/<hash>/jcode`, zet `~/.jcode/builds/current/jcode`
daarop, installeert de Doppler-launcher op `~/.local/bin/jcode` en zet
`[websearch] engine = "exa"` in `~/.jcode/config.toml` (met een getimestampte
backup). `verify.sh` start daarna twee tijdelijke servers (eigen runtime dir,
dus de server van de gebruiker blijft ongemoeid) en laat die de echte
`websearch`-tool uitvoeren via de debug-socket.

## Wat de binary is

```
$ env -u EXA_API_KEY jcode --version
jcode v0.64.168-dev (2c383fffc)

$ readlink ~/.jcode/builds/current/jcode
/home/maarten/.jcode/builds/versions/2c383fffc/jcode

$ ls -la ~/.local/bin/jcode
-rwxr-xr-x 1 maarten maarten 1253 ... /home/maarten/.local/bin/jcode     # wrapper, geen symlink
```

## Fase 1 — echte zoekopdracht, sleutel runtime uit Doppler

`verify.sh` start de server via de launcher (`/home/maarten/.local/bin/jcode`)
met `env -u EXA_API_KEY`, zodat de sleutel alleen via `exa-cli print-key`
→ Doppler `infra/all` binnen kan komen. Letterlijke uitvoer:

```
== jcode Exa-verificatie ==
launcher   : /home/maarten/.local/bin/jcode
binary     : /home/maarten/.jcode/builds/current/jcode (/home/maarten/.jcode/builds/versions/2c383fffc/jcode)
config     : engine = exa
sleutelbron: exa-cli/Doppler

== fase 1: echte zoekopdracht (sleutel runtime uit Doppler) ==
$ /home/maarten/.local/bin/jcode serve --socket /tmp/jcode-exa-verify.vXgGsy/run1/jcode.sock
sessie: session_maple_1789559844942_106fd9bcf7e7bc4d
$ /home/maarten/.local/bin/jcode debug -s /tmp/jcode-exa-verify.vXgGsy/run1/jcode.sock 'tool:websearch {"query":"Exa semantic search API"}'
Search results for: Exa semantic search API

1. **https://exa.ai/docs/reference/search**
   https://exa.ai/docs/reference/search
   > The search endpoint lets you search the web and extract contents from the results.
[... resultaat 1, 2 en 3 hier afgekapt: elke snippet is een echte
Exa-highlight van ±1000 tekens paginatekst (zie de cap van 1200 tekens in
parse_exa_results) ...]

provider: exa (requestId d76d48e8f7a40b65aee7a51c5580ee19)

OK: provider = exa

```

De regel `provider: exa (requestId d76d48e8f7a40b65aee7a51c5580ee19)` is het
bewijs: de Exa-`requestId` komt uit het antwoord van `POST
https://api.exa.ai/search`, dus de zoekopdracht is echt door Exa uitgevoerd.
Zonder `EXA_API_KEY` in de omgeving kan die regel niet verschijnen: de launcher
haalt de sleutel dan uit Doppler.

## Fase 2 — ontbrekende sleutel geeft een expliciete foutmelding

```
== fase 2: zonder sleutel hoort jcode duidelijk te klagen ==
$ env -u EXA_API_KEY /home/maarten/.jcode/builds/current/jcode serve --socket /tmp/jcode-exa-verify.vXgGsy/run2/jcode.sock
$ env -u EXA_API_KEY /home/maarten/.jcode/builds/current/jcode debug -s /tmp/jcode-exa-verify.vXgGsy/run2/jcode.sock 'tool:websearch {"query":"..."}'
Error: Exa engine selected but no API key is available. Set `websearch.exa_api_key` in your config, or export the EXA_API_KEY environment variable (the PC001 launcher fetches it from the Doppler `infra/all` secret).

✅ jcode gebruikt Exa; zonder sleutel volgt een expliciete foutmelding.
```

De server is hier direct uit de binary gestart (buiten de launcher om), dus er
is geen enkele weg waarlangs een sleutel binnenkomt; de tool faalt dan met de
boodschap hierboven in plaats van stil over te stappen op een fallback-engine.

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
