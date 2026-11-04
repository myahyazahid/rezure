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

Detected clients (see [`services/db_clients.rs`](../../src-tauri/src/services/db_clients.rs)):

| Client | Opens straight onto the database? | How it's launched |
|---|---|---|
| TablePlus | yes | `mysql://user:password@host:port/database` connection URL |
| DBeaver | yes | `-con driver=mariadb\|host=…\|database=…\|user=…\|save=false\|connect=true\|password=…` |
| HeidiSQL | yes — the tree shows only that database | `--host= --port= --user= --password= --databases=` |
| MySQL Workbench | server only | `--query user@host:port`; Workbench asks for the password itself |
| Navicat | server only | opens the app (no documented connection flags) |
| MariaDB console | yes | the bundled `mariadb.exe`, in a new console window |

"Server only" means the client has no command-line way to preselect a schema — it opens on
the right server, and you pick the database inside it. The menu labels these so a click
never looks like it silently opened the wrong thing.

The bundled console client is always offered last, so the menu is never empty on a machine
with no GUI installed. With exactly one client available, **Open** launches it directly
instead of showing a one-item menu.

Each entry shows the client's own logo, read out of its installed `.exe`
([`utils/exe_icon.rs`](../../src-tauri/src/utils/exe_icon.rs)), so it's always the logo of the
version you have. The consoles show the MariaDB or PostgreSQL mark.

DBeaver and Workbench take a few seconds to show a window. While a client starts, the page shows
**Opening <client>…** for at least three seconds, until Rezure's window loses focus to the new
one, or for at most 20 seconds. **Open** is disabled while it's up, so a second click doesn't
start a second copy.

### Remote connections and passwords

For a connection to another server, the client is pointed at the address Rezure itself uses.
For a connection through SSH, that's the tunnel's local port on `127.0.0.1`. That port changes
from one session to the next. The client never deals with SSH, so a connection that signs in
to SSH with a `.pem` key needs nothing extra.

The database password is a separate matter. TablePlus, DBeaver, HeidiSQL and the MariaDB console
connect as soon as they open, without asking for one, and fail with *Access denied … (using
password: NO)*. A password saved in the client wouldn't help either, because the tunnel port it
was saved for is gone next time. So when the connection has a saved password, Rezure hands it
over:

| Client | How the password is passed |
|---|---|
| TablePlus | In the URL, percent-encoded, so `@`, `:` or `/` in it can't break it |
| DBeaver | `password=` in the `-con` spec. Left out if it contains `\|` or `=`, which the spec can't carry |
| HeidiSQL | `--password=`. Left out if it starts with `-` or contains a space or `"`, which HeidiSQL can't read back |
| MariaDB console | `MYSQL_PWD`, set on the console process only, never on a command line |
| MySQL Workbench | Not passed. Workbench has no command-line option for it, so it asks in its own dialog |

A password on a command line can be read by other programs running as the same Windows user.
That's the same group that can already read it from Credential Manager, and without it **Open**
doesn't work for a remote server at all. Every other client, and every other place in Rezure,
keeps passwords off command lines.

When a password is left out for the reasons above, the client fails the way it did before.
Type the password into the client's own connection dialog.

### Adding another client

Detection is by install path — no registry scraping. Add an entry to `CANDIDATES` in
[`services/db_clients.rs`](../../src-tauri/src/services/db_clients.rs):

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
`MySQL Workbench 8.0 CE`). Then add the client's connection flags to `args_for`, and
set `opens_database` honestly — the UI relies on it. Its logo needs nothing: it's read out of
the `.exe` that was found.

Check the flags against the real client, not its `--help`. HeidiSQL reads `-db=x` as `-d`
(session name) `b=x`, and Workbench 8.0 treats `-query x` as a file named `x` to open. Long
`--name=` forms avoided both.

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

An import into the **local** server skips the binary log (`SET SESSION sql_log_bin = 0`, for
that import's connection only). MySQL 8 logs every change by default and keeps the logs for
30 days, so without this a 2 GB dump also wrote about 1 GB of `binlog.*` files that nothing on
a dev machine reads. The server's own binlog setting and every other connection are untouched.
Imports into a **remote** connection never do this — the binlog may feed a replica or a backup
there.

---

## Dropping a database

On the **local server**, each row has a trash-can button at the end of its actions. It opens a
dialog that names the database, its table count and size, warns when a project's site uses
it, and keeps **Delete database** disabled until you type `CONFIRM`. Dropping a schema can't
be undone, so it is deliberately more than one stray click away from the Export button next
to it — export first if you might want it back.

There is no Delete on a **connection** — a remote MySQL/MariaDB server, PostgreSQL, or SQL
Server. The page doesn't show the button (`ServerInfo::drop_supported` is only true for the
local server), and the `drop_database` command itself refuses whenever a connection is
active, so nothing else can reach it either. Drop those from a SQL client.

MariaDB's and MySQL's own `mysql`, `information_schema`, `performance_schema` and `sys`
schemas are never listed and can never be dropped.

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
[`services/database.rs`](../../src-tauri/src/services/database.rs) drives those instead of
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
- **Delete** (the bin icon on a row) asks for CONFIRM to be typed, as on MariaDB. It closes any
  open sessions first, so a PHP worker or Odoo holding a connection doesn't block it. It's offered
  on Rezure's own PostgreSQL only, never on a connection to someone else's server.

Every query goes through the `psql` of an installed PostgreSQL build, passing SQL on stdin:
Windows hands a console program its arguments in the ANSI code page, which turned `é` into an
invalid byte when passed on the command line.

### Users, and connecting an app

**Users** (next to New database, on Rezure's own PostgreSQL only) lists the logins and adds new
ones. The server comes with two superusers, `postgres` and `root`. That's enough for Laravel,
but some apps want a login of their own. Odoo refuses to run as `postgres`, and an app that will
one day meet a real server is better developed against its own user that owns its own database.

A new user can log in, and optionally:

- **Create a database it owns**. One app, one user, one database, which is how most apps expect
  to be set up.
- **May create databases of its own**. Odoo needs this, and so do Django's and Rails' test
  runners.

The presets (**App**, **Django / Rails**, **Odoo**) only fill these in. A password is optional:
Rezure's server lets local connections in without checking one. Set one anyway (**Generate**
makes one), so the app's settings keep working on a real server. Rezure doesn't store it.

**App config** on a user shows the settings to paste into an app so it logs in as that user. It
doesn't connect or change anything by itself. The settings come in the form each framework reads: a
`postgresql://` URL, Laravel's `.env`, `DATABASE_URL` for Django, Node and Prisma, Rails'
`database.yml`, JDBC, and `odoo.conf`. Right after creating a user they include the password
just typed. Later, Rezure no longer knows it, so the password is left blank.

A user that owns databases can't be deleted until those databases are. Anything finer than
this (grants, schemas) is a SQL client's job.

### Odoo

Odoo's Windows installer can bundle its own PostgreSQL, but the box isn't always tickable, and
Odoo then installs with no database server. When Rezure finds Odoo under Program Files, the
Databases page (on Rezure's PostgreSQL) says so, and **Connect Odoo** points it at Rezure's
server with one administrator prompt:

1. Creates the `odoo` login, allowed to create databases.
2. Sets `db_host`, `db_port`, `db_user`, `db_password` and `pg_path` in `odoo.conf`. Without
   `pg_path`, Odoo's database manager can't find `pg_dump` for backups. The original file is kept
   as `odoo.conf.rezure-backup`. Every other line is left as it was. A block of `;` comments
   (in Indonesian) goes right under `[options]`. It explains what each of those lines is for,
   where the master password is (`admin_passwd`), and that changes need an Odoo restart.
   Connecting again replaces the block instead of adding a second copy. Odoo drops it when it
   rewrites the file itself, for example after the master password is changed in its database
   manager.
3. Restarts Odoo's service, if it's running.
4. Optionally stops Odoo's own PostgreSQL service and sets it to Manual, since both servers use
   port 5432. If that service is running, this step is required.

Odoo starts with Windows and expects its database straight away, while Rezure's PostgreSQL only
runs while Rezure does. The dialog therefore puts two settings in one place: **Start
PostgreSQL when Rezure opens** and **Start Rezure with Windows**. Rezure is meant to stay open
while Odoo is in use. Keep in tray, under Settings, is what lets its window be closed without
quitting it. Odoo 20's installer ships PostgreSQL 16, and Rezure's catalog has 16 too. The dialog
says so when the active version is another major.

**"Database management function blocked, bad admin password"** in Odoo's database manager is
about Odoo's *Master Password*, not the database login. It's `admin_passwd` in `odoo.conf`, set
by Odoo's installer, and Rezure leaves it as it is. The dialog says which state it's in, without
showing the value:

- Not set: it's `admin`.
- Plain text: open `odoo.conf` to read it.
- Hashed (`$pbkdf2-sha512$…`): it can't be read back. Replace the value with a new password,
  editing `odoo.conf` as administrator, then restart Odoo's service.

**Open**, next to the file's path, opens `odoo.conf` in Notepad run as administrator, after a
UAC prompt. The file is under Program Files, so a Notepad without admin rights couldn't save it.
Below it, Odoo's Windows service has its own row: **Stop** and **Restart** while it runs,
**Start** while it doesn't. Restart is what makes Odoo read `odoo.conf` again after an edit.
Start and Restart bring Rezure's PostgreSQL up first, since Odoo is no use without it.

Controlling a service normally takes admin rights, so each click asks. **Allow without asking**
asks once, and gives your Windows account permission to start, stop and query that one service.
It can't change the service. From then on the buttons work without a prompt. **Put back Odoo's
own settings** takes the permission away again.

**"role "odoo" does not exist"** means `odoo.conf` points at Rezure's PostgreSQL, but the login
it names isn't there. The Odoo row on the Databases page turns red, and the dialog says so
first. **Recreate it** makes the login again from the user name and password already in
`odoo.conf`, with no admin prompt and no change to the file. Starting or restarting Odoo from
the dialog does the same check every time.

Databases already made in Odoo's own PostgreSQL stay there, and Rezure doesn't move them.
**Put back Odoo's own settings** restores the original `odoo.conf` and turns Odoo's PostgreSQL
service back on. Before that, it stops Rezure's PostgreSQL, so the port is free.

### A PostgreSQL elsewhere

Add it as a connection (Add connection → Server type: PostgreSQL). It needs at least one
PostgreSQL version installed here, for `psql` and `pg_dump`; the newest one is used, because
`pg_dump` refuses a server newer than itself. **Encryption** maps to libpq's `sslmode`: Off is
`disable`, Automatic is `prefer`, Required is `require`. A password is passed through a
temporary password file, never on a command line.

Open in a client, for PostgreSQL:

| Client | How it's opened |
|---|---|
| TablePlus | `postgresql://user:password@host:port/database`, with the saved password as for MySQL (see [Remote connections and passwords](#remote-connections-and-passwords)) |
| DBeaver | `-con "driver=postgres-jdbc\|…"`, with the saved password as for MySQL. The first time, DBeaver asks to download its PostgreSQL JDBC driver |
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
  with EDB's installer, or the one Odoo brings, runs as a Windows service on 5432. The service
  then won't start, and its card names the program holding the port. When that program is a
  Windows service, the card names the service and offers to stop it. Windows asks for
  administrator permission once. If the service starts with Windows, a checkbox also sets it to
  Manual so it doesn't take the port back after a restart.

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

**"Access denied … (using password: NO)" when opening a remote database** — the connection has
no saved password, or it's one the client can't be handed (see
[Remote connections and passwords](#remote-connections-and-passwords)). Save the password on
the connection in Rezure, or type it into the client's connection dialog.

**HeidiSQL shows a session named like `b=shop`** — a leftover from an older Rezure, which
passed `-db=`. Delete it in HeidiSQL's session manager. Current versions don't create it.

**Sizes look wrong** — the Size column is `data_length + index_length` from
`information_schema`, which is a storage estimate for InnoDB, not an exact byte count. A
freshly created or freshly imported database can read as `0` until MariaDB updates its
statistics. On MySQL 8+ those numbers are also cached for a day
(`information_schema_stats_expiry`); Rezure turns that cache off for its own query, so Refresh
shows the current size rather than the one from when the first tables appeared.
