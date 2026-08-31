# Rezure — MySQL Profile Switcher

## 1. Background

Rezure currently bundles its own MySQL instance and datadir. Some users (including the primary use case) already have an existing MySQL datadir from another local dev tool (e.g. Laragon, XAMPP) containing large databases (10GB+) that they do not want to export/import.

**Constraint:** a single `mysqld` process can only read/write one datadir at a time. Two MySQL server processes must never point at the same datadir simultaneously — this risks data corruption via conflicting locks on InnoDB files.

**Chosen approach:** single-instance "Profile Switcher". Rezure still runs exactly one `mysqld` process at any given time, but the user can switch which datadir (and matching MySQL binary version) that process points to. Switching stops the current server, points it at the new datadir, and restarts it. Only one profile is ever "live" at a time.

This is explicitly **not** a multi-instance architecture (no parallel mysqld processes on different ports) and **not** a physical merge of two datadirs into one folder. Each profile keeps its own datadir untouched.

## 2. Data Model

Each profile is a record with the following fields:

```json
{
  "id": "uuid-v4",
  "name": "Laragon",
  "datadir_path": "C:/laragon/data/mysql",
  "mysql_version": "8.0.30",
  "port": 3306,
  "source": "laragon",
  "is_default": false,
  "last_used_at": "2026-08-30T10:00:00Z"
}
```

- `id`: stable unique identifier, independent of name/path (name/path may be edited later).
- `name`: display name shown in the switcher UI. User-editable.
- `datadir_path`: absolute path to the MySQL datadir this profile points to.
- `mysql_version`: MySQL version string that created this datadir (detected or user-confirmed). Used to pick a compatible bundled binary.
- `port`: port to bind the server to when this profile is active. Defaults to 3306 but should be configurable per profile in case the user wants to run something else on 3306 concurrently.
- `source`: one of `"rezure" | "laragon" | "xampp" | "custom"`. Purely informational/for icon+label in UI; does not affect logic.
- `is_default`: true for exactly one profile — the one Rezure falls back to on first run / fresh install. This is the bundled Rezure datadir, created and owned by Rezure itself.
- `last_used_at`: for sorting the switcher list by recency.

All profiles are stored in a single config file, e.g. `%APPDATA%/Rezure/profiles.json`, as an array of the above objects, plus a top-level `active_profile_id` field recording which profile is currently live.

On first install, `profiles.json` is seeded with exactly one profile: the Rezure-owned default datadir, `is_default: true`, marked active.

## 3. Auto-detection (optional, nice-to-have)

A scan routine checks common install locations for other local dev tools:

- Laragon: default install path is typically `C:\laragon`. Its MySQL config lives at `laragon\bin\mysql\mysql-<version>\my.ini`. Read the `datadir` value from that file, and infer `mysql_version` from the folder name (`mysql-8.0.30` → `8.0.30`).
- XAMPP: default install path is typically `C:\xampp`. Config at `xampp\mysql\bin\my.ini`, datadir usually `xampp\mysql\data`. Version is harder to infer from path alone — may need to read `mysqld --version` output from the bundled binary, or a version file if present.

When a tool is detected, prompt the user: *"Laragon detected — add its MySQL data as a profile?"* before creating anything. Never silently add a profile.

This step is optional relative to the core switcher — the switcher must work fully via **manual** "Add Profile" (user browses to a folder + confirms/enters the MySQL version) even if auto-detection is not implemented yet or fails to find anything.

## 4. UI

### 4.1 Switcher control

Add a profile switcher — a dropdown/pill button — near the top of the **Databases** page (and/or in the sidebar), showing the currently active profile's name, e.g. `Profile: Rezure Default ▾`.

Clicking it opens a list of all saved profiles plus an **"Add Profile"** action at the bottom (opens a folder picker + a form for name / version / port / source).

### 4.2 Active profile indicator

Wherever the "SERVER" info bar is shown (see existing `127.0.0.1:3306 · root · no password` bar in the Databases page), also show which profile is currently active, so the user always knows where "New database" will land before they click it.

### 4.3 Add / Edit profile form

Fields: Name, Data directory (folder picker), MySQL version (dropdown of bundled versions, or free text + validation), Port, Source (optional label). On save, do **not** touch the datadir contents in any way — only record the path.

## 5. Switch Logic

When the user selects a different profile from the switcher:

1. **Confirm** if there are unsaved/pending operations (e.g. an import in progress) — block switch if so.
2. **Stop** the currently running `mysqld` process cleanly (graceful shutdown, not kill -9, to avoid leaving the datadir in an inconsistent state).
3. **Resolve binary**: check `mysql_version` on the target profile against the set of MySQL binaries bundled with Rezure. If no exact match exists, pick the closest compatible one (see §6) or block the switch with a clear error if nothing compatible is bundled.
4. **Start** `mysqld` with `--datadir=<target.datadir_path> --port=<target.port>` (plus any other flags Rezure normally passes).
5. **Verify** the new server actually came up (poll a health check / attempt a connection) before declaring the switch successful.
6. **Update** `active_profile_id` in `profiles.json` and `last_used_at` on the target profile.
7. **Refresh** the Databases page by reconnecting and re-querying `SHOW DATABASES` against the newly active server.

If step 4 or 5 fails, roll back: attempt to restart the previous profile's server so the user isn't left with no running MySQL at all, and surface a clear error message.

## 6. Version Compatibility

MySQL datadirs are generally **not** forward/backward compatible across major versions (e.g. a datadir created by 5.7 cannot simply be opened by an 8.0 binary without running `mysql_upgrade`, and it's not guaranteed even then; going backward — newer binary's datadir opened by an older binary — is unsupported and can corrupt data).

Rules to enforce:

- Store the profile's `mysql_version` and compare its major.minor against the bundled binary set.
- **Exact major.minor match** → safe, just start normally.
- **Same major, different minor/patch** (e.g. datadir is 8.0.30, Rezure only bundles 8.0.36) → generally safe; MySQL minor/patch upgrades within the same major line are typically backward-compatible for the datadir. Proceed but log a note.
- **Different major** (e.g. 5.7 vs 8.0) → do **not** attempt automatically. Block the switch and show the user an explicit warning explaining the risk, with an option to run `mysql_upgrade` manually first, or bundle the older major version binary as well if Rezure wants to support it out of the box.
- Bundling multiple major versions (e.g. keep 5.7.x and 8.0.x binaries both available) avoids forcing the user to upgrade anything — Rezure just launches whichever binary matches the target profile's declared version.

## 7. Safety Checks

- **Before switching TO a profile that isn't Rezure's own** (e.g. Laragon's): check whether the *other* application (Laragon) currently has a MySQL process running against that same datadir. If detected (e.g. a lock file or PID file inside the datadir, or a process holding the port), block the switch and show: *"Laragon's MySQL appears to be running — stop it there first before switching Rezure to this profile."*
- **Never** allow two profiles pointing at the same `datadir_path` to be active-adjacent in a way that could race (e.g. don't let a switch proceed if the previous stop hasn't fully completed).
- Do not delete, move, or write into a profile's datadir except by starting `mysqld` against it. No copying, no merging.
- Consider a lightweight advisory lock file written by Rezure itself inside a profile's datadir while active, removed on clean stop, so Rezure can detect "was this profile shut down cleanly last time" and warn on next switch-to if not.

## 8. Explicitly Out of Scope (for this feature)

- Running multiple `mysqld` processes concurrently (multi-instance architecture) — rejected in favor of this single-instance switcher.
- Physically merging two datadirs into one folder — not supported; each profile's datadir stays wherever it already is.
- Cross-version live migration of a datadir — the version compatibility rules above only gate *whether a switch is allowed*, they do not perform any migration themselves.
