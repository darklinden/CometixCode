# CometixCode task runner.
#
# Tests run under `cargo nextest`, which gives each test its own process.
# Measured on one commit, no test-code changes:
#
#   cargo test --lib                        530 failed   15s
#   cargo test --lib + the env pinning      522 failed   15s
#   cargo nextest run --lib                  23 failed  132s
#   just test                                19 failed  129s
#
# Nearly all of it is nextest: inside one process 164 files call `set_var` and
# 200 share `TEST_ENV_LOCK`, where one panic while holding it poisons the lock
# for every later test that unwraps it. Pinning the environment cannot fix
# tests stepping on each other; separate processes can. The cost is ~8x wall
# clock, which is worth it for a result you can act on.
#
# The env pinning below is worth the remaining handful, in both directions:
#   CLAUDE_CONFIG_DIR  redirects the settings root AND ~/.claude.json
#                      (utils/config.rs:119-120), so tests stop asserting
#                      against the developer's own config.
#   privacy vars       gate analytics, which decides whether growthbook
#                      features are read at all (services/analytics/config.rs:6).
#                      A dev shell exporting CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC
#                      turned those tests into no-ops that passed for the wrong
#                      reason.
#   CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR
#                      short-circuits `reset_cwd_if_outside_project`
#                      (bash_tool/utils.rs:184) to always report the project
#                      root, whatever the shell actually did. A dev shell
#                      exporting it made `foreground_shell_reports_persistent_
#                      physical_cwd` fail against correct product behaviour —
#                      it read as a cwd bug for as long as it was in the
#                      known-failures list.
# It is recipe-scoped, so `cargo run` still uses the real config.
#
# What the pinning does NOT cover: `projectSettings`. Settings merge from five
# sources (utils/settings/mod.rs:1-8) and CLAUDE_CONFIG_DIR only moves the USER
# one. `projectSettings` is `${cwd}/.claude/settings.json`, rooted at
# `get_original_cwd()` — process state, not an env var, so no recipe can pin it.
# A test that reads settings therefore reads THIS repository's
# `.claude/settings.json`, which configures hooks; that is how a
# `UserPromptSubmit` assertion in process_user_input silently picked up the
# repo's own Trellis hook and failed. Tests that care must pin it themselves
# with `bootstrap::state::set_original_cwd(...)` plus a Drop guard that restores
# it — point it at the tracked `tests/fixtures/isolated-project`, the same
# hook-free workdir used by the recipes. `IsolatedProjectSettings` is the
# reference guard.
#
# Mutable harness state lives under `target/test-home`, so a clean checkout has
# every immutable input and `cargo clean` reliably resets generated state. CC
# puts BOTH the settings root and the global config file under CLAUDE_CONFIG_DIR when it is set
# (utils/env.ts:25
# `join(process.env.CLAUDE_CONFIG_DIR || homedir(), '.claude.json')`), even
# though ~/.claude.json is a sibling of ~/.claude/ by default — so one variable
# really does cover both.

home    := justfile_directory() / "target/test-home"
project := justfile_directory() / "tests/fixtures/isolated-project"

# A fixed mock identity in the scratch home. Credentials are a FILE
# (`.credentials.json` under get_config_home, i.e. under CLAUDE_CONFIG_DIR), and
# on macOS the keychain backend is `#[cfg(all(target_os = "macos", not(test)))]`
# — it does not even compile into the test binary, so `fallback_storage` always
# lands on the plain-text file. One mock file therefore gives every test a
# deterministic logged-in identity on every platform, instead of reading
# whatever the developer happens to be logged into.
#
# Tests that need the LOGGED-OUT case must clear it themselves; that is their
# state to establish, not the harness's default.
#
# The identity is a checked-in synthetic fixture rather than a heredoc, so it
# is visible and reviewable. COMETIX_TEST_PROJECT_DIR closes the projectSettings
# hole described above: `${cwd}/.claude/settings.json` resolves inside the
# tracked hook-free fixture instead of this repository.
iso  := 'CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC= DISABLE_TELEMETRY= NODE_ENV= CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR= COMETIX_TEST_PROJECT_DIR=' + quote(project)
# Third pinning layer: model/endpoint/auth variables the HOST Claude Code
# session injects via its --settings env block (verified 2026-08-19: a host
# profile sets ANTHROPIC_BASE_URL / ANTHROPIC_AUTH_TOKEN /
# ANTHROPIC_DEFAULT_{OPUS,SONNET,HAIKU}_MODEL / CLAUDE_CLASSIFIER_MODEL, which
# 26 `_like_official` model/auth assertions then read instead of the built-in
# defaults — task #49/#50). These need `env -u`, NOT the `VAR=` empty-string
# form the privacy vars use: the model resolvers take `env::var(..)` without
# an is-empty filter (utils/model/model.rs:24-26), so an empty string would be
# adopted as a model NAME. Tests that need one of these set establish it
# themselves with EnvVarGuard.
#
# Second class in the same layer: the host's build/profile gates. Nothing here
# is named MODEL or TOKEN, so a leak does not look like one — it silently flips
# a branch, and the failure surfaces as a MISSING piece rather than a leaked
# variable. CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS=1 alone made
# should_use_global_cache_scope() (utils/betas.rs:162) false and took six
# prompt/cache-boundary assertions down, each reporting only its own absent
# marker; CLAUDE_CODE_USE_{BEDROCK,VERTEX,FOUNDRY} move get_api_provider()
# (utils/model/providers.rs:14) off FirstParty, gating that same function plus
# the beta headers. All four are read with a bare `env::var(..)`, so `env -u`
# again.
unhost := 'env -u ANTHROPIC_MODEL -u ANTHROPIC_SMALL_FAST_MODEL -u ANTHROPIC_DEFAULT_OPUS_MODEL -u ANTHROPIC_DEFAULT_SONNET_MODEL -u ANTHROPIC_DEFAULT_HAIKU_MODEL -u CLAUDE_CODE_MODEL -u CLAUDE_CODE_SMALL_FAST_MODEL -u CLAUDE_CODE_SUBAGENT_MODEL -u CLAUDE_CODE_DEFAULT_OPUS_MODEL -u CLAUDE_CODE_DEFAULT_SONNET_MODEL -u CLAUDE_CODE_DEFAULT_HAIKU_MODEL -u CLAUDE_CLASSIFIER_MODEL -u COMETIX_MODEL -u ANTHROPIC_CUSTOM_MODEL_OPTION -u ANTHROPIC_CUSTOM_MODEL_OPTION_NAME -u ANTHROPIC_CUSTOM_MODEL_OPTION_DESCRIPTION -u ANTHROPIC_BASE_URL -u ANTHROPIC_AUTH_TOKEN -u ANTHROPIC_API_KEY -u CLAUDE_CODE_USE_BEDROCK -u CLAUDE_CODE_USE_VERTEX -u CLAUDE_CODE_USE_FOUNDRY -u CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS'
nt := 'cargo nextest run --lib --no-fail-fast'

# Rebuild generated state and serialize the one trusted project through a JSON
# encoder; path text is never interpolated into JSON syntax.
[private]
_prep:
    #!/usr/bin/env python3
    import json, pathlib, shutil
    root = pathlib.Path.cwd()
    home = root / "target/test-home"
    project = (root / "tests/fixtures/isolated-project").resolve(strict=True)
    shutil.rmtree(home, ignore_errors=True)
    home.mkdir(parents=True)
    shutil.copyfile(root / "tests/fixtures/credentials.mock.json", home / ".credentials.json")
    payload = {"projects": {str(project): {"hasTrustDialogAccepted": True}}}
    (home / ".claude.json").write_text(json.dumps(payload, sort_keys=True, separators=(",", ":")) + "\n")

default:
    @just --list

# Type/borrow check, tests included. The edit-loop companion to `just test`:
# it links nothing, so it is the fast way to close a refactor, but it proves
# only that the code compiles — it is NOT a gate.
#
# No env pinning: nothing here executes a test body, so the host state the
# test recipes pin cannot be read.
check *args:
    cargo check --lib --tests {{args}}

# Same, for the binary and its release profile.
check-release *args:
    cargo check --release {{args}}

# Lint gate. `--all-targets` covers the `#[cfg(test)]` code too, like `check`'s
# `--lib --tests`, plus the `[[bin]]` that `check` leaves out. It executes no
# test body, so it needs none of the env pinning `test` does. The crate-level
# allows that preserve the 1:1 TS-port shapes (nested `if`s, CC function
# signatures, ...) live in Cargo.toml's `[lints.clippy]` table, each with its
# reason written next to it.
#
# Only the `external` audience — `just lint-ant` is the other half.
lint *args:
    cargo clippy --all-targets {{args}} -- -D warnings

# Full suite — the gate. Trustworthy, so batches no longer scope themselves to
# the directly-touched modules.
test *args:
    @just _prep
    CLAUDE_CONFIG_DIR={{quote(home)}} {{iso}} {{unhost}} {{nt}} {{args}}

# Quick substring filter: `just t state::store`
t pattern:
    @just _prep
    CLAUDE_CONFIG_DIR={{quote(home)}} {{iso}} {{unhost}} {{nt}} -E {{quote("test(/" + pattern + "/)")}}

# Full nextest filterset: `just test-one 'binary(cometix-code) and test(foo)'`
test-one filter:
    @just _prep
    CLAUDE_CONFIG_DIR={{quote(home)}} {{iso}} {{unhost}} {{nt}} -E {{quote(filter)}}

# nextest does not run doctests.
test-doc:
    cargo test --doc

# --- Anthropic-internal build profile ---------------------------------------
#
# CC gates ~510 sites on `USER_TYPE === 'ant'`, which its bundler substitutes at
# build time (`"external" === 'ant'` in the external source). Cometix models it
# as a Cargo feature (utils/build_profile.rs). The recipes above only ever build
# `external`, which leaves a hole in the gate that the env pinning above does
# not cover: the audience is a THIRD axis, next to host state and process
# isolation.
#
# Measured 2026-08-05:
#   131  production gate sites
#    50  `#[cfg(feature = "anthropic_internal")]`  — NOT COMPILED by `just check`
#    13  `cfg!(feature = "anthropic_internal")`    — compiled, branch never taken
#    29  tests that exist only under the feature   — invisible to `just test`
#
# The 50 attribute-gated sites are the real hazard: a signature change in shared
# code cannot break them visibly, because rustc never sees them. Run `just
# check-ant` and `just lint-ant` after any cross-cutting refactor; run `just
# test-ant` before claiming a batch is green.
check-ant *args:
    cargo check --lib --tests --features anthropic_internal {{args}}

# `lint`'s ant twin. Unlike the `check-ant` above, this keeps `lint`'s
# `--all-targets` so the `[[bin]]` is covered under the feature too.
lint-ant *args:
    cargo clippy --all-targets --features anthropic_internal {{args}} -- -D warnings

test-ant *args:
    @just _prep
    CLAUDE_CONFIG_DIR={{quote(home)}} {{iso}} {{unhost}} {{nt}} --features anthropic_internal {{args}}

# Substring filter under the internal build: `just t-ant sandbox_permission`.
# `test-ant *args` word-splits, so a filterset containing `(` needs this form.
t-ant pattern:
    @just _prep
    CLAUDE_CONFIG_DIR={{quote(home)}} {{iso}} {{unhost}} {{nt}} --features anthropic_internal -E {{quote("test(/" + pattern + "/)")}}

# Both audiences, for a release-shaped gate. Runs BOTH even when the first
# fails — a bare `just test; just test-ant` aborts on the first non-zero exit,
# which would silently skip the audience this recipe exists to cover.
test-all-audiences:
    #!/usr/bin/env bash
    set -uo pipefail
    rc=0
    just test || rc=1
    just test-ant || rc=1
    exit $rc

# EMPTY as of 2026-08-05. `just test` is expected to be fully green; a red test
# is a red test.
#
# The list held five. NONE was a product defect — every one was a broken fixture
# or a leaked host variable, and each had been sitting behind a note telling the
# next person not to look at it:
#
#   validation_error_is_reported_as_non_blocking_failure
#     Fed syntactically invalid JSON and asserted on the schema-validation path.
#     CC separates the two: `parseHookOutput` catches a `jsonParse` throw and
#     returns bare `{ plainText }` (utils/hooks.ts:447-450), so malformed JSON
#     is ordinary output; only valid JSON failing the schema returns
#     `validationError` (:446). Rust already matched CC — the payload was wrong.
#   cwd_changed_updates_dynamic_paths_restarts_and_notifies
#   file_event_updates_dynamic_paths_and_reports_failed_output
#     No workspace-trust fixture, so both measured the hook executor's untrusted
#     early return (env.rs:165-175). "Expected 2 results, got 0" read as a
#     product bug for as long as the note stood.
#   foreground_shell_reports_persistent_physical_cwd
#     CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR leaked from the developer's
#     shell and short-circuits reset_cwd_if_outside_project. Now cleared in
#     `iso`.
#   status_notice_context_uses_official_api_key_approval_for_conflict_source
#     `api_key_source` resolves through `load_global_config()` and the process
#     environment (auth.rs:534), not the arguments the test passed. The sibling
#     test in the same file documented this and injected both fixtures; this one
#     did neither.
#
# So: a failure that survives a rerun is a defect somewhere — in the product, the
# fixture, or the harness. Find which. Do not re-open this list to park it.
#
# Load-sensitive flakes (repl::permissions_*, repl_context_command,
# shell_command::pipe_mode) are a separate matter: they pass on an idle machine
# and fail under contention, so compare same-filter runs on a quiet machine
# before calling one a regression.
known-failures := ''

test-known-failures:
    @echo 'known-failures is empty — `just test` should be green. See the justfile note.'
