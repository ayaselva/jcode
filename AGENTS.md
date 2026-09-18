# Repository Guidelines

Lees de centrale agent-instructies [/data/ai/AGENTS.md](/data/ai/AGENTS.md) volledig als die nog niet in deze sessie is geladen; deze file vult ze aan voor `/data/worktrees/jcode/exa-websearch`.

## Development Workflow

- **Commit as you go** - Make small, focused commits after completing each feature or fix
- If the git state is not clean, or there are other agents working in the codebase in parallel, do your best to still commit your work. 
- **Push when done** - Push all commits to remote when finishing a task or session
- **Run the guardrails before pushing** - `scripts/check_guardrails.sh` runs every gate in
  CI's Format + Quality Guardrails jobs (fmt, clippy `-D warnings`, and the warning,
  code-size, test-size, panic, swallowed-error, dependency-boundary, and wildcard-reexport
  ratchets). Use `--skip-slow` to skip cargo check/clippy, and `--fix` to rustfmt and
  rebaseline ratchets after intentional growth. CI tracks the `stable` toolchain, so run
  `rustup update stable` too: a stale local clippy passes on lints that CI enforces.
- **Use fast iteration by default** - Prefer `cargo check`, targeted tests, and dev builds while iterating
- **Rebuild when done** - When you are done making changes, build the source.
- **Bump version for releases** - Update version in `Cargo.toml` when making releases. When cutting a new release, look at all the changes that happened since the last release and determine what the version bump should be ie patch or minor, etc. 
- **Remote builds available** - Use `scripts/remote_build.sh` to offload heavy cargo work to another machine. If your build is terminated, likely is because there are not enough resources on this machine to build. use remote build in that case. Try checking the resource avaliablity on the machine before you run a build. 

## Logs
- Logs are written to `~/.jcode/logs/` (daily files like `jcode-YYYY-MM-DD.log`).

## Debug Socket
- Use the debug socket for runtime level debugging

## Install Notes
- `~/.local/bin/jcode` is the launcher symlink used from `PATH`.
- `~/.jcode/builds/current/jcode` is the active local/source-build channel; self-dev builds and `scripts/install_release.sh` point the launcher here.
- `~/.jcode/builds/stable/jcode` is the stable release channel; `scripts/install.sh` installs this and points the launcher here.
- `~/.jcode/builds/versions/<version>/jcode` stores immutable binaries.
- `~/.jcode/builds/canary/jcode` still exists for canary/testing flows, but it is not the primary self-dev install path.
- On Windows, the equivalents are `%LOCALAPPDATA%\\jcode\\bin\\jcode.exe` for the launcher, `%LOCALAPPDATA%\\jcode\\builds\\stable\\jcode.exe` for stable, and `%LOCALAPPDATA%\\jcode\\builds\\versions\\<version>\\jcode.exe` for immutable installs; `scripts/install.ps1` currently installs the stable channel.
- Ensure `~/.local/bin` is **before** `~/.cargo/bin` in `PATH`.

## PC001: Exa als websearch-provider

Deze branch (`pc001-exa-websearch`) voegt Exa toe als standaard zoekengine van de
`websearch`-tool en levert de PC001-bedrading onder `pc001/`.

- **Scope** — alleen de websearch-keten: `WebSearchEngine::Exa` +
  `WebSearchConfig.exa_api_key(_env)` (`crates/jcode-config-types`), de
  env-overrides/allowlist/het configsjabloon/de redactielijst
  (`crates/jcode-base`), de engine zelf (`crates/jcode-app-core/src/tool/websearch.rs`)
  en de scripts in `pc001/`. Geen upstream-vendor: `upgrade.sh` haalt upstream
  (`https://github.com/1jehuang/jcode.git`) on demand op.
- **Sleutel** — `EXA_API_KEY` komt runtime uit Doppler `infra/all` via de
  gedeelde `exa-cli`-helper en leeft alleen in het procesmilieu van
  `~/.local/bin/jcode` (wrapper uit `pc001/jcode-launcher`). Nooit in een
  bestand, commit, log of op de opdrachtregel; verifieer met
  `env -u EXA_API_KEY` dat de sleutel echt via Doppler komt.
- **Werkwijze** — wijzigingen in een eigen git-worktree
  (`git -C /data/projects/jcode worktree add ...`), nooit in de dirty
  hoofdboom; mergen naar `master` pas als die schoon is.
  Installeer/publiceren: `pc001/install.sh`; upstream bijwerken:
  `pc001/upgrade.sh`.
- **Verificatie** — `pc001/verify.sh` start een tijdelijke jcode-server uit de
  geïnstalleerde binary en laat die een echte Exa-zoekopdracht uitvoeren; de
  output moet `provider: exa (requestId ...)` bevatten. Zonder sleutel moet
  dezelfde tool een expliciete foutmelding geven. Letterlijke commando's en
  uitvoer staan in `pc001/docs/VERIFICATIE.md`.
- **Guardrails** — draai `scripts/check_guardrails.sh` (`cargo fmt --check`,
  clippy `-D warnings`, ratchets) voordat je pusht. Let op: de
  grootte-ratchets (1200 regels per bestand) gelden ook voor
  `crates/jcode-app-core/src/tool/websearch.rs`.

