# Database profiles

Rezure runs **one** database server at a time. A *profile* decides which data directory that
server is pointed at — Rezure's own, or one belonging to a tool you already use.

This is what lets you open a Laragon or XAMPP database in Rezure without exporting and
re-importing gigabytes of data. Nothing is copied, moved, merged, or converted: a profile
records a path.

---

## Why a switcher and not several servers

A running server owns its data directory exclusively. Two server processes against the same
folder is the one reliable way to corrupt InnoDB. So Rezure keeps exactly one server running
and makes the *data directory* the thing that changes.

That rules out, by design:

- running several servers side by side on different ports,
- merging two data directories into one,
- migrating a data directory between versions.

---

## Two engines, and why both

| Tool | Engine it ships |
|---|---|
| Rezure | MariaDB |
| XAMPP | MariaDB |
| Laragon | Oracle MySQL |

A data directory written by one **cannot** be opened by the other. The projects forked at
MySQL 5.5, and MySQL 8.0+ keeps a data dictionary (`mysql.ibd`) MariaDB has no reader for.
Supporting only one engine would mean refusing half the data directories on a typical
machine, so Rezure supports both.

The abstraction is thin, because MariaDB's Windows build ships `mysqld.exe`, `mysql.exe`,
`mysqladmin.exe` and `mysqldump.exe` as compatibility aliases beside its own `mariadb*`
names. Every binary Rezure invokes is spelled the same on both engines. The only genuine
difference is how an empty data directory gets bootstrapped:

| Engine | Bootstrap |
|---|---|
| MariaDB | `mariadb-install-db.exe --datadir=…` |
| MySQL 8.0+ | `mysqld --initialize-insecure --datadir=…` |

That only ever runs for a directory Rezure creates. An adopted one is opened as-is.

---

## Adding a profile

The switcher sits at the top of the **Databases** page, showing the active profile's name.
Open it and choose **Add data directory**.

Rezure scans the usual locations first:

- **Laragon** — reads `datadir` out of each `C:\laragon\bin\mysql\<version>\my.ini`. The path
  isn't derivable from the folder name (`mysql-8.4.3-winx64` uses `data\mysql-8.4`), so the
  ini is read rather than guessed.
- **XAMPP** — the fixed `C:\xampp\mysql\data`, with the version read from the binary itself.

Anything already registered is filtered out. Nothing is ever added without you clicking
**Add**.

If neither is found, **Point at a folder myself** takes a folder picker.

### What Rezure records

| Field | Where it comes from |
|---|---|
| Data directory | the folder you picked |
| Engine | read from the folder's own marker files, never from your answer |
| Version | read from the server binary's `--version` |
| Server binary | the build that came with the data — see below |
| `my.ini` | the config that build launches with — see below |
| Port | yours to choose; defaults to 3306 |

Profiles live in `%APPDATA%\Rezure\profiles.json`.

### The engine is detected, not asked

Picking the wrong engine for a data directory is the mistake that corrupts it, so Rezure
doesn't rely on you knowing:

| Marker file in the data directory | Engine |
|---|---|
| `mysql.ibd` | MySQL 8.0+ (its data dictionary) |
| `aria_log_control` | MariaDB (Aria is MariaDB-only) |

Both engines leave an `ibdata1`, which is why that shared file is no help and isn't consulted.
If neither marker is present the folder isn't treated as a data directory at all.

### The binary is referenced, not copied

An adopted profile runs on **the build that came with its data** — Laragon's MySQL 8.4 data
directory needs Laragon's own MySQL 8.4 binary, and that build is already on disk.

Copying it would duplicate a quarter of a gigabyte to no purpose, and it's consistent with
what a profile already is: a pointer to somebody else's folder. If that tool is uninstalled
its data directory goes with it, so an owned copy of the binary would have outlived the only
data it could open.

### The config is carried too — this matters

A profile also records the `my.ini` its install launches with, and Rezure passes it as
`--defaults-file`.

This is not a nicety. XAMPP's `my.ini` sets `plugin_dir`; without it the server can't load
Aria, and an Aria `mysql.db` privilege table then reads as `Incorrect file format 'db'` and
startup aborts. Laragon and XAMPP both launch their servers with `--defaults-file` for exactly
this reason, so adopting their data means adopting their config.

Profiles saved before Rezure recorded this are healed automatically on startup — the `my.ini`
is found beside the recorded binary (XAMPP keeps it in `mysql\bin`, Laragon one level up).

---

## Switching

Selecting a different profile runs one sequence, in this order:

1. **Gate.** Everything that could destroy data is checked *before* anything is stopped, so a
   refused switch costs you nothing. See below.
2. **Stop cleanly.** The running server is asked to shut down and given time to flush.
3. **Point and start.** Binary, data directory and port are all re-resolved from the newly
   active profile at spawn time.
4. **Verify.** Rezure waits until the server actually accepts connections. A process that was
   created is not the same as a server that came up.
5. **Roll back on failure.** If the new profile won't start, the previous one is made active
   again and restarted, so a failed switch leaves a working database rather than none.

### The gate

A switch is refused, with a specific reason, when:

| Check | Why |
|---|---|
| The data directory was written by the other engine | It cannot be opened at all |
| No compatible binary is installed | See version rules below |
| A live server already holds that data directory | A second server against it corrupts it |

The last check reads the **process table**, not the `.pid` file in the data directory. A pid
file is unreliable in both directions: one machine had Laragon's data directory holding a pid
file naming a long-dead process, and a force-killed server leaves a pid file that outlives it.
Rezure asks every running `mysqld`/`mariadbd` what `--datadir` it was started with, and
compares paths.

### Version compatibility

Data directories are not freely portable between versions.

| Relationship | Verdict |
|---|---|
| Same `major.minor`, different patch (8.0.30 → 8.0.36) | Allowed |
| Same major, different minor (8.0 → 8.4, MariaDB 10.4 → 10.11) | **Refused** |
| Different major (8.4 vs 9.6, MariaDB 10 vs 11) | **Refused** |
| Version unknown | Refused — never assumed compatible |

> **Stricter than the original spec, deliberately.** The spec called "same major, different
> minor" generally safe. That's true for a *patch* bump, which is what its example showed —
> but a minor bump like MySQL 8.0 → 8.4 rewrites the data directory on first start and
> **cannot be undone**. Since the whole premise is opening data another tool owns, an
> irreversible rewrite is not something to do on a logged note.

If a profile is refused for this reason, the fix is to install a binary matching its
`major.minor`, not to force it.

---

## Stopping

The database is asked to shut down cleanly (`mysqladmin shutdown`) and given **30 seconds**
before being force-killed. That's generous on purpose: a clean InnoDB shutdown flushes dirty
pages, and the whole point of profiles is pointing at data directories measured in gigabytes.
A timeout tuned for Rezure's own near-empty directory would force-kill exactly the large,
valuable ones.

**Force stop** (the ⋯ menu on the service row) skips that wait and kills immediately. It exists
because a hung server would otherwise hold you for the full timeout with no way out. For a
database it asks first, because the data directory is then left needing crash recovery.

---

## What follows the active profile

Everything, resolved at the time it's needed rather than cached:

- the server binary, data directory and port that get spawned,
- the port and client binaries the Databases page queries with — so a MariaDB client is never
  pointed at a MySQL server,
- the name and version on the service card, which reads **MySQL** when a MySQL profile is
  active.

---

## Safety guarantees

- A data directory is never read, written, copied or merged by Rezure — only opened by starting
  a server against it. Bootstrap runs only on a folder that is empty.
- Two profiles cannot point at the same data directory. Paths are compared case-insensitively,
  ignoring slash direction and trailing separators, so one folder can't be registered twice
  under two spellings.
- The Rezure-owned profile can't be deleted; it's the fallback every failed switch rolls back
  to. Neither can the active one.
- Removing a profile forgets a path. It never touches the folder.

---

## Troubleshooting

**"this datadir was written by X, but the profile says Y"**
The folder's own marker files disagree with the profile. Remove the profile and add it again so
the engine is re-detected.

**"no MySQL 8.4 binary is installed"**
The profile names a build Rezure can't find — usually because the tool that owned it was
uninstalled or moved. Re-add the profile so it picks up the build's current location.

**"Laragon's database server looks like it's still running"**
Stop it there first. Two servers against one data directory is the case this exists to prevent.

**`Incorrect file format 'db'` on startup**
The server can't read the `mysql` privilege tables. Either the config it needs isn't being
passed (see *The config is carried too* above), or the data directory was already in this state
before Rezure touched it — check that tool's own error log for when it last started
successfully. Rezure aborts at this point without writing to your data.

**The switch succeeded but the Databases page is empty**
The server came up against a different data directory than you expected. Check which profile
the switcher shows as active; "New database" lands wherever that points.

---

## Not supported

- Running two servers at once.
- Merging data directories.
- Migrating a data directory across versions — the rules above gate *whether a switch is
  allowed*, they never perform an upgrade.

---

The original design note that preceded this feature is kept at
[`mysql-profile-switcher-spec.md`](mysql-profile-switcher-spec.md); where the implementation
departs from it, the reasoning is given above.
