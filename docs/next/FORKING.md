# Cubix fork development guide

This fork (DarkPhilosophy/herdr, future name: **Cubix**) tracks
`ogulcancelik/herdr` as upstream but follows its own development line with
features upstream has not accepted or has not implemented yet.

## Branches

- `develop` — main integration branch. Tracks `upstream/master` plus fork
  commits. All work lands here first.
- `master` (local) — kept for reference; do not commit directly.
- `feat/*`, `fix/*` — short-lived task branches, merged into `develop`.

## Syncing with upstream

```bash
git fetch upstream
git checkout develop
git merge upstream/master
# resolve conflicts, then:
just check
```

Sync regularly (weekly or before starting a large feature) to keep the diff
surface small.

## Build environment

Two toolchains are required, both user-local (no dnf/system packages):

- **Rust** via rustup (`~/.cargo`), pinned by `rust-toolchain.toml`.
- **Zig 0.15.x** at `~/.local/bin/zig` for the vendored libghostty-vt build.
  Install:
  ```bash
  curl -L https://ziglang.org/download/0.15.2/zig-x86_64-linux-0.15.2.tar.xz | tar -xJ -C ~/.local/share
  ln -sf ~/.local/share/zig-x86_64-linux-0.15.2/zig ~/.local/bin/zig
  ```

### Zig global cache must live on a real path

On this machine `/home` is a symlink to `/var/home` (Bazzite/Atomic). Zig
0.15.2's build runner fails to spawn cache-built executables
(`failed to spawn and capture stdio ... FileNotFound`) when the global cache
resolves through that symlink. Always export:

```bash
export ZIG_GLOBAL_CACHE_DIR=/var/home/alexa/.cache/zig-real
```

Add it to your shell profile; without it every `cargo build` fails at the
libghostty-vt step.

## Validation

Always run before committing:

```bash
just check   # rustfmt check + cargo nextest + maintenance script tests
```

`cargo-nextest` is required (`cargo install cargo-nextest --locked`).

## Adding a new agent

Agent identity is data-driven via `AGENT_REGISTRY` in `src/detect/mod.rs`:

1. Add one enum variant to `Agent` **at the end** (registry order must match
   enum declaration order; `registry_stays_in_enum_declaration_order` tests
   this).
2. Add one `AgentInfo` entry to `AGENT_REGISTRY` (label, executable, aliases,
   `has_screen_manifest`).
3. If the agent has detectable screen chrome, add
   `src/detect/manifests/<label>.toml` with declarative state rules and
   register it in `BUNDLED_MANIFESTS` in `src/detect/manifest.rs`.

That is all: `Agent::ALL`, labels, executables, alias lookup, sound config
keys (`ui.sound.agents.<label>`), and manifest loading derive from the
registry. Do **not** hand-write per-agent match arms.

Detection manifests support `contains`, `regex`, `line_regex`, nested
`all`/`any`/`not` gates, priorities, regions, and `visible_*` flags. Copy an
existing manifest (e.g. `maki.toml`, `droid.toml`) as a starting point and
capture real screen output before writing rules (see `AGENTS.md`: detection
is evidence-based).

Users can also drop local override manifests without recompiling; see
`ManifestSource::Override` in `src/detect/manifest.rs`.

## Config schema changes

The maintenance tests enforce that `src/config/*.rs` and
`docs/next/website/src/data/config-reference.json` stay in sync:

- Enumerable keys must appear in the JSON reference.
- Open-ended subtrees (dynamic keys such as `ui.sound.agents`,
  `ui.sidebar.sections`, `keys.command`) must be listed in
  `SKIPPED_SUBTREES` in `scripts/config_reference_check.py` **and**
  documented in prose in `docs/next/website/src/content/docs/configuration.mdx`.

## Integration assets

Files under `src/integration/assets/` carry `HERDR_INTEGRATION_VERSION`
markers with a matching `*_INTEGRATION_VERSION` const in
`src/integration/mod.rs`. Bump once per release when the asset changes
(version is relative to the latest released tag, not per-commit).

## Commit style

Lowercase conventional commits, no emojis, no AI co-author lines. When a
change relates to an upstream issue, add `refs #<n>` in the body (never
`fixes`/`closes` — upstream release CI owns issue closing).

## Rebranding status

The product name is **Cubix** (chosen, not yet applied). The technical
rename (binary name, crate name, config dir, socket paths, docs) is a
dedicated later phase; until then the codebase stays `herdr`-named to keep
upstream merges trivial.
