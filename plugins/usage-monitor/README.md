# usage-monitor

Sidebar section with one usage bar per provider limit (Claude, Codex, Devin, ...).

## Data source

Read-only from omp's `usage_history` table (`~/.omp/agent/agent.db`, override with `OMP_AGENT_DB`). The plugin never reads `~/.claude` or `~/.codex` credentials and never calls a provider API, so it cannot rotate or invalidate a login.

Snapshots are only written while omp runs:

- newer than `max_age_secs` (default 30 min): shown live;
- older but within `grace_secs` (default 6 h): last bar, dimmed, with its age;
- older still: a one-line "stale" note, never a bar.

Limits that already reset are dropped.

## Install

```sh
cd plugins/usage-monitor
cargo build --release          # manifest starts target/release/herdr-usage-monitor
mkdir -p ~/.config/herdr/plugins/config/herdr-usage-monitor
cp config.example.toml ~/.config/herdr/plugins/config/herdr-usage-monitor/config.toml
```

Then register the directory as a local plugin (see herdr's plugin docs), and add a custom section to `~/.config/herdr/config.toml`:

```toml
[[ui.sidebar.sections]]
id = "usage"
title = "Usage"
max_rows = 20      # two rows per limit; raise it when you add providers
```

## Apply changes

| Changed | Do |
|---|---|
| sidebar section (`max_rows`, ...) | `herdr server reload-config` |
| plugin `config.toml` | restart the daemon: `herdr-usage-monitor --stop`, then `--start` |
| plugin code | `cargo build --release`, then restart the daemon |

`cargo clean` removes the binary; the sidebar section disappears until you rebuild.

## Config

Every key is optional. Providers are omp ids, so nothing is hardcoded.

| Key | Meaning |
|---|---|
| `providers` | ids to show, in order; empty shows every provider with fresh data |
| `labels` | id -> display label |
| `max_age_secs`, `grace_secs`, `poll_secs`, `section_id` | timing and section id |

Environment overrides: `USAGE_POLL_INTERVAL`, `USAGE_MAX_AGE_SECS`, `USAGE_GRACE_SECS`, `USAGE_SECTION_ID`, `OMP_AGENT_DB`.

## Modes

`--start` (idempotent) · `--stop` (also clears the section) · `--refresh` · `--once` (print payload, no publish) · `--daemon`.

Errors appear as a row in the section and in `herdr-usage-monitor.log` in the plugin state directory.
