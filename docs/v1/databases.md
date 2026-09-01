# Databases

The Databases page manages the schemas inside **Rezure's own MariaDB** — creating them,
exporting them, importing dumps, and handing one off to whichever SQL client you already
use.

---

## What Rezure's MariaDB is (and isn't)

Rezure runs its **own** MariaDB server with its own data directory:

| | |
|---|---|
| Binary | `C:\rezure\bin\mariadb\<version>\...\bin\mariadbd.exe` |
| Data directory | `C:\rezure\data\mariadb\data` |
| Bootstrapped by | `mariadb-install-db`, on first start |

It is **not** connected to a Laragon, XAMPP, or system-wide MySQL install. Those keep their
databases in their own data directories, and Rezure never reads or writes them. Starting
Rezure's MariaDB therefore shows an empty server on a fresh install — that's the expected
state, not data loss.

To bring existing databases over, dump them from the other tool and
[import the `.sql`](#importing-a-dump) here. Pointing Rezure at another tool's data
directory is not supported: MySQL 8.x data files can't be read by MariaDB at all, and an
older MariaDB data directory gets **irreversibly upgraded in place** the first time a newer
server opens it.

> **Port 3306 is shared.** Rezure, Laragon and XAMPP all default to it, so only one of them
> can run at a time. Starting a second one fails with a port-in-use error.

---

## Connecting

The page shows the connection once, at the top, with a **Copy DSN** button:

```
mysql://root@127.0.0.1:3306
```

**There is no root password.** That's deliberate, not an oversight: the server binds to
`127.0.0.1` only, and requiring a password for a throwaway local database would just move
the secret into a config file that has to be shared with every client anyway. Rezure never
prompts for credentials because it already knows them.

If you need a password, set one with `SET PASSWORD` through any client — but note that
Rezure's own commands connect as `root` with no password and will start failing.

---

## Opening a database in a SQL client

Rezure doesn't bundle a database GUI. You already have a favourite, with your own saved
queries and layout, and a half-clone of it inside Rezure would be one more thing to learn.
So **Open** detects what's installed and lets you choose.

Detected clients (see [`services/db_clients.rs`](../src-tauri/src/services/db_clients.rs)):

| Client | Opens straight onto the database? | How it's launched |
|---|---|---|
| TablePlus | yes | `mysql://` connection URL |
| DBeaver | yes | `-con driver=mariadb\|host=…\|database=…\|save=false\|connect=true` |
| HeidiSQL | server only | `-h= -P= -u=` |
| MySQL Workbench | server only | `-query root@127.0.0.1:3306` |
| Navicat | server only | opens the app (no documented connection flags) |
| MariaDB console | yes | the bundled `mariadb.exe`, in a new console window |

"Server only" means the client has no command-line way to preselect a schema — it opens on
the right server, and you pick the database inside it. The menu labels these so a click
never looks like it silently opened the wrong thing.

The bundled console client is always offered last, so the menu is never empty on a machine
with no GUI installed. With exactly one client available, **Open** launches it directly
instead of showing a one-item menu.

### Adding another client

Detection is by install path — no registry scraping. Add an entry to `CANDIDATES` in
[`services/db_clients.rs`](../src-tauri/src/services/db_clients.rs):

```rust
Candidate {
    id: "myclient",
    name: "My Client",
    locations: &[r"$PROGRAMFILES\MyClient\myclient.exe"],
    opens_database: true,
},
```

`$LOCALAPPDATA`, `$PROGRAMFILES` and `$PROGRAMFILESX86` are expanded at lookup time, and a
single `*` matches one path segment (for versioned install folders like
`MySQL Workbench 8.0 CE`). Then add the client's connection flags to `connection_args`, and
set `opens_database` honestly — the UI relies on it.

---

## Exporting

**Export** on a row runs `mariadb-dump` and writes a timestamped file to:

```
C:\rezure\dumps\<database>-<YYYYMMDD-HHMMSS>.sql
```

A fixed, documented folder rather than a save dialog: an export is usually one step of
"dump it, then do something with the file", and a predictable path is easier to reach from
a terminal afterwards. The timestamp is UTC. **Show folder** in the confirmation opens it in
Explorer.

If the dump fails, the partial file is deleted rather than left behind looking like a
successful export.

## Importing a dump

**Import .sql** opens a file picker, then asks which database to import into. The name is
pre-filled from the filename (Rezure's own export timestamp is stripped, so re-importing
your own dump suggests the original name).

- If the database doesn't exist, it's **created** first.
- If it does exist, the dump is applied on top. Most dumps contain `DROP TABLE IF EXISTS`,
  so tables the dump defines get replaced — the dialog warns about this.

Import is not transactional. A dump that fails halfway leaves the database partially
imported.

---

## Dropping a database

**There is no drop button in the UI, by design.** Dropping a schema is irreversible with no
undo, and it isn't an action worth putting one stray click away from a row you were only
trying to export. Drop a database from a SQL client, or from the bundled console:

```
DROP DATABASE `my_app`;
```

The `drop_database` command still exists in the Rust backend (guarded so MariaDB's own
`mysql`, `information_schema`, `performance_schema` and `sys` schemas can never be dropped),
so re-exposing it is a UI change only.

---

## The "Used by" column

Rezure matches a schema to a project by name, treating `_` and `-` as the same character —
a folder can't be named `shop_api` and produce `shop-api.test`, while a schema named
`shop-api` needs backticks in every hand-written query, so the two spellings drift apart for
the same project. `shop_api` therefore matches the project `shop-api` and shows
`shop-api.test`. A database with no matching project shows `—`.

This is a display convenience only. Rezure does not configure, inject, or enforce which
database a project actually connects to — that stays in your project's own `.env`.

---

## How it works under the hood

There is **no MySQL driver crate** in Rezure's dependency tree. The MariaDB zip Rezure
already downloads ships its own client binaries next to the server, so
[`services/database.rs`](../src-tauri/src/services/database.rs) drives those instead of
adding a second, redundant way to speak the protocol:

- `mariadb.exe --batch --skip-column-names -e "<sql>"` for every read, parsed as TSV
- `mariadb-dump.exe` for exports
- `mariadb.exe <db> < dump.sql` for imports

### Identifier safety

Database and collation names are **not** parameterizable — `CREATE DATABASE ?` isn't valid
SQL — so the only defence is to refuse anything that isn't a plain identifier before it
reaches the client. `validate_identifier` allows ASCII letters, digits, `_` and `-`, up to
64 characters, and rejects everything else. This is deliberately stricter than MySQL's own
backtick-quoted rules: nothing a local dev database legitimately needs is excluded, and no
quoting question can arise downstream. It's covered by tests that feed it statement-breaking
input.

Every argument handed to a SQL client is passed as its own argv entry — nothing is spliced
into a shell command line.

---

## Troubleshooting

**"MariaDB isn't running"** — the page says so with a link to Services, rather than showing
a raw client error. Start MariaDB there and retry.

**Port 3306 already in use** — Laragon or XAMPP is running. Stop it; the two can't share the
port.

**A client opens but connects to nothing** — for HeidiSQL, Workbench and Navicat this is
expected on first launch; they open on the server (or just open), and you pick the database
inside. Use **Copy DSN** if the client wants a connection string.

**Sizes look wrong** — the Size column is `data_length + index_length` from
`information_schema`, which is a storage estimate for InnoDB, not an exact byte count. A
freshly created or freshly imported database can read as `0` until MariaDB updates its
statistics.
