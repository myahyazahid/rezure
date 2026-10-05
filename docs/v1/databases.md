# Databases

The Databases page manages the schemas inside **Rezure's own MariaDB** — creating them,
exporting them, importing dumps, and handing one off to whichever SQL client you already
use. The same page reads [SQL Server](#sql-server) and [PostgreSQL](#postgresql), local or
elsewhere, through its connection switcher.

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

The local server always listens on the same address:

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
| HeidiSQL | yes — the tree shows only that database | `-h= -P= -u= -db=` |
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

## SQL Server

For projects whose database is Microsoft SQL Server. Everything below is installed from
inside Rezure, on request — but two of the pieces are real **Windows installs**, not files
under `C:\rezure`:

| Piece | Where it's installed from | What it is |
|---|---|---|
| `pdo_sqlsrv` / `sqlsrv` | PHP Extensions page, or a project's requirements check | Microsoft's PHP drivers. A DLL in the PHP version's `ext\`, like `redis` |
| Microsoft ODBC Driver 18 | PHP Extensions page, the requirements check, or Add connection | What both the PHP drivers and the Databases page talk to SQL Server through. **One UAC prompt**; listed in Settings → Apps |
| SQL Server Express LocalDB (optional) | Services card, or Switch → Install version | A local SQL Server. **One UAC prompt**; listed in Settings → Apps |

Before either Windows install, Rezure shows Microsoft's license and waits for you to tick
the box accepting it — it never accepts it for you. The installers are downloaded from
Microsoft, checked against a pinned checksum and Microsoft's signature, then run.
Uninstalling Rezure doesn't remove them.

**The PHP drivers load without the ODBC Driver** — PHP reports nothing wrong — and every
connection then fails with "This extension requires the Microsoft ODBC Driver for SQL
Server". The PHP Extensions page and the requirements check both say so up front.

### LocalDB

Rezure creates one instance, `Rezure`, as soon as LocalDB is installed. It has **no TCP
port**: clients reach it over a named pipe, as `(localdb)\Rezure`, signed in as your Windows
user. The Services card shows that address where other services show a port, and LocalDB
starts by itself the moment something connects — Start on the card is optional.

```dotenv
DB_CONNECTION=sqlsrv
DB_HOST='(localdb)\Rezure'
DB_PORT=           # must be empty — Laravel appends any port, even its 1433 default
DB_DATABASE=my_app
DB_USERNAME=       # empty = Windows Authentication
DB_PASSWORD=
```

**For SQL clients that can't open a named pipe** — TablePlus and DBeaver only speak to SQL
Server over TCP — Rezure also relays `127.0.0.1:14330` to the instance's pipe while it runs
(not 1433, so a SQL Server you install later can still have that). Connect with the SQL login
**`rezure`, no password**, and **encryption off**: over its pipe LocalDB offers no TLS, so a
client that insists on it is refused. The login is Rezure's, created on first use, with the
same stance as MariaDB's passwordless `root`: a throwaway local server, reachable from this
machine only. The Services card shows the port beside `(localdb)\Rezure`.

A connection called **SQL Server LocalDB** appears in the Databases switcher by itself. It's
treated like the local MariaDB: writable, with the "Used by" column. Databases created or
restored there keep their files in `C:\rezure\data\mssql\` (LocalDB's own default is the root
of your user folder). **Export** writes a `.bak` backup to `C:\rezure\dumps\`; **Import**
takes either a `.sql` script (split on `GO` lines, as SSMS does) or a `.bak`, which is
**restored whole**, replacing a database of the same name.

### A SQL Server elsewhere

Add it as a connection (Add connection → Server type: SQL Server). Sign in with a SQL Server
login, or with your Windows account for a server on your office domain. For a named instance
(`host\SQLEXPRESS`) leave the port empty.

**Instances already on this machine are found for you.** Choosing SQL Server in the form lists
what's installed here under **Found on this computer** — Express, Developer and other editions
that run as a Windows service, and LocalDB instances other than Rezure's own. Click one and the
form fills in the host, Windows account sign-in, and (for an installed edition, which has a
self-signed certificate) **Trust server certificate**. Nothing is saved until you test it and
press Save, and an instance you've already added isn't offered again. The scan is read-only: it
never starts a service or creates an instance
([`services/mssql_discovery.rs`](../../src-tauri/src/services/mssql_discovery.rs)).

Unlike the MariaDB scan for Laragon and XAMPP, this offers a *connection*, not a data
directory: a SQL Server install is a Windows service Rezure doesn't run, so there's no folder for
it to adopt.

Export and `.bak` restore aren't offered for these: `BACKUP` and `RESTORE` read and write the
**server's** disk, not this machine's. Listing, creating, dropping (when the connection isn't
read-only) and running `.sql` scripts work.

**ODBC Driver 18 encrypts by default** and refuses a self-signed certificate, which most
development and office servers have. Tick **Trust server certificate** on the connection for
such a server. A Laravel project needs the same thing in two places — uncomment
`'trust_server_certificate'` in `config/database.php` **and** set
`DB_TRUST_SERVER_CERTIFICATE=true` — because Laravel ships that config line commented out, so
the env var alone does nothing. LocalDB doesn't need any of this.

Open in a client, for SQL Server:

| Client | How it's opened | When it's offered |
|---|---|---|
| SQL Server Management Studio | `-S`, `-d`, `-E`/`-U` | Always — it reaches LocalDB's pipe itself |
| TablePlus | `sqlserver://user@host:port/database` | A SQL login on a known port — for LocalDB, the bridge and `rezure`. TablePlus can't do Windows Authentication |
| DBeaver | `-con "driver=microsoft\|…"`, with `prop.encrypt=false` for LocalDB | Same as TablePlus. The first time, DBeaver asks to download its SQL Server JDBC driver — once per DBeaver workspace |

HeidiSQL isn't offered for SQL Server: version 12.8 opened its session manager on the last
session for every form of its MSSQL command line (`-n=4`) that was tried, and never connected.
It's still offered for MySQL and MariaDB.

`show databases` doesn't exist in SQL Server — use `SELECT name FROM sys.databases`.

---

## PostgreSQL

A local PostgreSQL, run by Rezure like MariaDB — beside it, on its own port, not instead of
it. Install it from its card on the Services page (the newest version) or from Switch →
Install version (18, 17 or 16). These are EDB's portable Windows builds, unpacked under
`C:\rezure\bin\postgres\<version>\`: no installer, no admin prompt, nothing in Settings → Apps.
Only the server and its tools are kept — about 150 MB — not the pgAdmin that comes in the same
archive.

| | |
|---|---|
| Address | `127.0.0.1:5432` |
| Users | `postgres` and `root`, both superusers |
| Password | none needed — the server trusts connections from this machine, and only listens on `127.0.0.1` |
| Data | `C:\rezure\data\postgres\<major>\`, created on the first start |

`root` is there because it's the username Laravel's `config/database.php` falls back to for
`pgsql`, so a project switched to PostgreSQL connects without touching `DB_USERNAME`:

```dotenv
DB_CONNECTION=pgsql
DB_HOST=127.0.0.1
DB_PORT=5432
DB_DATABASE=my_app
DB_USERNAME=root
DB_PASSWORD=
```

**PHP's `pdo_pgsql` is off by default** — it ships with every PHP build, it just isn't loaded —
and Laravel then fails with "could not find driver". Turn it on in PHP Extensions, or from a
project's requirements check, which also says whether PostgreSQL is installed and running and
whether `.env` matches it.

### Versions and their data

The Switch page picks which installed version the service runs. **Each major version keeps
its own databases**: a data directory can only be opened by the major that created it, so
switching from 17 to 18 starts 18 on its own data — nothing is upgraded or lost, 17's data is
still there when you switch back. Move databases between them with Export and Import. Switching
while PostgreSQL is running stops it cleanly and starts the new version.

PostgreSQL refuses to run as administrator. If Rezure itself was opened with "Run as
administrator", Start says so — close it and open it normally.

### On the Databases page

A connection called **PostgreSQL** appears in the switcher by itself, treated like the local
MariaDB: writable, with the "Used by" column.

- **New database** asks for an encoding rather than a collation, and creates it in UTF-8 from
  `template0`.
- **Export** runs `pg_dump` and writes a plain `.sql` file to `C:\rezure\dumps\`, without
  ownership or privilege statements, so it imports into a database of any name, under any role.
  It can't be cancelled half-way.
- **Import** runs a `.sql` file with `psql`, creating the database first if it doesn't exist,
  and stops at the first error, naming its line. A `pg_dump` script has no `DROP` statements
  unless it was taken with `--clean`, so importing into a database that already has those tables
  stops at the first one — import into a new name instead.
- **Drop** (from the backend; there's still no button) closes any open sessions on Rezure's own
  server first, so a PHP worker holding a connection doesn't block it.

Every query goes through the `psql` of an installed PostgreSQL build, passing SQL on stdin:
Windows hands a console program its arguments in the ANSI code page, which turned `é` into an
invalid byte when passed on the command line.

### A PostgreSQL elsewhere

Add it as a connection (Add connection → Server type: PostgreSQL). It needs at least one
PostgreSQL version installed here, for `psql` and `pg_dump`; the newest one is used, because
`pg_dump` refuses a server newer than itself. **Encryption** maps to libpq's `sslmode`: Off is
`disable`, Automatic is `prefer`, Required is `require`. A password is passed through a
temporary password file, never on a command line.

Open in a client, for PostgreSQL:

| Client | How it's opened |
|---|---|
| TablePlus | `postgresql://user@host:port/database` |
| DBeaver | `-con "driver=postgres-jdbc\|…"`. The first time, DBeaver asks to download its PostgreSQL JDBC driver |
| pgAdmin 4 | Opens the app only — it has no command line to pass a connection |
| psql console | `psql` from an installed build, in its own window. Always offered |

HeidiSQL isn't offered for PostgreSQL until its command line has been checked against a real
server, the same reason it isn't offered for SQL Server.

---

## Several database servers at once

MySQL (or MariaDB), SQL Server LocalDB and PostgreSQL are three separate services, each on its
own address. Any of them can run at the same time, and one project can use more than one.

| Server | Service id | Address | Sign in as |
|---|---|---|---|
| MySQL or MariaDB | `mariadb` | `127.0.0.1:3306`, or the active profile's port | `root`, no password (Rezure's own data folder) |
| SQL Server LocalDB | `sqlserver` | `(localdb)\Rezure`; `127.0.0.1:14330` for tools that only speak TCP | Windows Authentication; over TCP, `rezure` with no password |
| PostgreSQL | `postgres` | `127.0.0.1:5432` | `root` or `postgres`, no password |

What doesn't run side by side:

- **MySQL and MariaDB take turns.** There's one MySQL-family server at a time, and the active
  [profile](database-profiles.md) decides which. That has no effect on SQL Server or
  PostgreSQL.
- **The Databases page reads one server at a time**, the one picked in the switcher. Picking
  another only changes what the page lists — nothing is stopped.
- **A port another program already holds.** Laragon and XAMPP use 3306. A PostgreSQL installed
  with EDB's installer runs as a Windows service on 5432. The service then won't start, and its
  card names the program holding the port.

Each server keeps its own memory. Stop the ones you aren't using, or take them off the
Services page with **Manage services**.

### From PHP

Each server needs its PDO driver loaded in the PHP that serves the project:

| Server | Driver | In Rezure |
|---|---|---|
| MySQL / MariaDB | `pdo_mysql` | On by default |
| SQL Server | `pdo_sqlsrv` | Installed from PHP Extensions, and needs the Microsoft ODBC Driver |
| PostgreSQL | `pdo_pgsql` | Ships with PHP but is off by default — turn it on in PHP Extensions |

With plain PDO, against Rezure's own servers:

```php
$mysql = new PDO('mysql:host=127.0.0.1;port=3306;dbname=my_app', 'root', '');
$pgsql = new PDO('pgsql:host=127.0.0.1;port=5432;dbname=my_app_reports', 'root', '');
// No user and password: Windows Authentication.
$mssql = new PDO('sqlsrv:Server=(localdb)\Rezure;Database=my_app_legacy');
```

### From Laravel

**Laravel's `config/database.php` reads the same variables for every connection** — `mysql`,
`pgsql` and `sqlsrv` all take `DB_HOST`, `DB_PORT`, `DB_USERNAME`, `DB_PASSWORD` and `DB_URL`.
One set of `DB_*` values can't describe three servers, so `DB_CONNECTION` and `DB_*` stay for
the default connection, and every other connection gets variables of its own in
`config/database.php`:

```php
'pgsql' => [
    'driver' => 'pgsql',
    'url' => env('PG_URL'),
    'host' => env('PG_HOST', '127.0.0.1'),
    'port' => env('PG_PORT', '5432'),
    'database' => env('PG_DATABASE', 'laravel'),
    'username' => env('PG_USERNAME', 'root'),
    'password' => env('PG_PASSWORD', ''),
    // charset, prefix, search_path, sslmode: as Laravel ships them
],

'sqlsrv' => [
    'driver' => 'sqlsrv',
    'url' => env('MSSQL_URL'),
    'host' => env('MSSQL_HOST', '(localdb)\Rezure'),
    'port' => env('MSSQL_PORT', ''),         // LocalDB has no port: keep it empty
    'database' => env('MSSQL_DATABASE', 'laravel'),
    'username' => env('MSSQL_USERNAME', ''), // empty = Windows Authentication
    'password' => env('MSSQL_PASSWORD', ''),
    // charset, prefix, prefix_indexes: as Laravel ships them
],
```

```dotenv
DB_CONNECTION=mysql        # the default connection
DB_HOST=127.0.0.1
DB_PORT=3306
DB_DATABASE=my_app
DB_USERNAME=root
DB_PASSWORD=

PG_DATABASE=my_app_reports
MSSQL_DATABASE=my_app_legacy
```

Then name the connection wherever it isn't the default:

```php
DB::connection('pgsql')->table('reports')->get();

class LegacyOrder extends Model
{
    protected $connection = 'sqlsrv';
}
```

A migration for another connection sets the same `protected $connection` property, so a single
`php artisan migrate` creates each table on its own server.

A project's requirements check reads `DB_CONNECTION`, so it checks the default connection
only. For the others, check their driver on the PHP Extensions page.

---

## Troubleshooting

**"MariaDB isn't running"** — the page says so with a link to Services, rather than showing
a raw client error. Start MariaDB there and retry.

**Port 3306 already in use** — Laragon or XAMPP is running. Stop it; the two can't share the
port.

**A client opens but connects to nothing** — for Workbench and Navicat this is
expected on first launch; they open on the server (or just open), and you pick the database
inside. If the client wants a connection string, use the one under [Connecting](#connecting).

**Sizes look wrong** — the Size column is `data_length + index_length` from
`information_schema`, which is a storage estimate for InnoDB, not an exact byte count. A
freshly created or freshly imported database can read as `0` until MariaDB updates its
statistics.
