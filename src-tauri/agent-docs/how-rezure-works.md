# How Rezure works

Rezure is a desktop app for Windows that runs a local web development stack
(Nginx, PHP, MariaDB/MySQL, Mailpit), similar to Laragon or XAMPP. This guide is
for AI coding agents and developers working on projects Rezure serves. It
describes Rezure {{version}}, installed at `{{home}}`.

`{{home}}\AGENTS.md` lists the current state of this machine: the active PHP
version, the database, and every project with its URL and PHP version. Read it
first.

**If `C:\laragon` or `C:\xampp` also exists on this machine, that doesn't mean
they serve the projects.** Rezure's projects are served by Rezure. See
[Coexisting with Laragon](#coexisting-with-laragon-and-xampp).

## Services and ports

| Service | Process | Listens on |
|---|---|---|
| Nginx | `nginx.exe` | `0.0.0.0:80`, plain HTTP. There is no HTTPS. |
| PHP (active version) | 4 × `php-cgi.exe` | FastCGI on `127.0.0.1:9100`–`9103` |
| PHP (a version a project pins) | 4 × `php-cgi.exe` each | `127.0.0.1:9110`–`9113`, then `9120`–`9123`, ... |
| Database (MariaDB or MySQL) | `mysqld.exe` | `127.0.0.1:3306`, or the active profile's port |
| Mailpit | `mailpit.exe` | SMTP `127.0.0.1:1025`, web inbox `http://127.0.0.1:8025` |
| SQL Server LocalDB (optional) | `sqlservr.exe`, run by LocalDB itself | A named pipe, addressed as `(localdb)\Rezure`; Rezure also relays `127.0.0.1:14330` to it |
| PostgreSQL (optional) | `postgres.exe` (one process per connection) | `127.0.0.1:5432` |

- Services are started and stopped **from the Rezure app** (Services page: Start
  all / Stop all / Restart all, or each service's own button). Rezure starts
  nothing on its own when it opens, and stops everything when it quits.
- A service missing from the Services page was removed there with **Manage
  services**. It's still installed; adding it back from the same button
  restores its card.
- Don't start, stop or kill these processes yourself. Rezure tracks them by PID
  file (`{{home}}\data\<service>\service.pid`) and regenerates their config
  before each start. A process started by hand won't have that config.
- To check what's running without the app:
  `Get-Process nginx, php-cgi, mysqld, mailpit, postgres -ErrorAction SilentlyContinue`.
- PHP is restarted automatically if it crashes (with backoff; it gives up after 5
  crashes in 5 minutes). Nginx and the database are not.
- If a port is taken by another program (IIS or Laragon on 80, another MySQL on
  3306), the service won't start. The Services page names the program holding the
  port and can free it.

## Folder layout

| Path | What it is |
|---|---|
| `{{home}}\www\` | Projects. Each top-level folder is one project. |
| `{{home}}\bin\<runtime>\<version>\` | Runtimes Rezure downloaded (php, nginx, mariadb, postgres, node, composer, mailpit, cloudflared). |
| `{{home}}\custom\<runtime>\<name>\` | Runtimes added by hand. They're used, but Rezure didn't verify them. |
| `{{home}}\current\php` | A junction to the active PHP version's folder. |
| `{{home}}\data\` | Runtime state: generated Nginx and PHP config, the default database's data, PID files. |
| `{{home}}\etc\` | Settings (`settings.json`), database profiles (`profiles.json`), linked projects (`links.json`), the CA bundle (`cacert.pem`), and **PHP overrides (`php\conf.d\`)**. |
| `{{home}}\dumps\` | SQL exports made from the Databases page. |
| `{{home}}\rezure.db` | Rezure's own SQLite database (project history, PHP/Node version pins). |

## Projects

- A project is a folder in `{{home}}\www\`. Its URL is `http://<folder-name>.test`.
  The `.test` suffix is fixed. A folder whose name can't be a domain gets no site.
  Projects created from the app use lowercase letters, digits and hyphens.
- A folder elsewhere on disk can also be **linked** as a project (Projects page).
  Linked projects are recorded in `{{home}}\etc\links.json` with their own
  domain. Rezure never writes into a linked folder.
- New projects can be created from templates: Laravel (`composer create-project`),
  WordPress, blank PHP, static HTML.
- **Document root:** a Laravel project (one with an `artisan` file and a
  `public\` folder) is served from `public\`. Everything else is served from the
  project folder itself. Requests fall through to `index.php` (front controller).
- **Hosts file:** `.test` domains resolve through a block Rezure manages in
  `C:\Windows\System32\drivers\etc\hosts`, between
  `# --- Rezure managed entries (do not edit below) ---` and
  `# --- Rezure managed entries end ---`. Writing it needs admin rights, so it
  only happens when the user clicks **Sync hosts** on the Projects page (a UAC
  prompt), or at startup if that's turned on in Settings. **Creating a project
  doesn't add its hosts entry.** If `<name>.test` doesn't resolve, that's why.
- **Requirements check** on a project card checks the project's PHP extensions,
  outgoing HTTPS from PHP, and its mail settings.

## Web server (Nginx)

- Rezure generates Nginx's config from scratch **on every start and reload**:
  `{{home}}\data\nginx\nginx.conf`, one `{{home}}\data\nginx\vhosts\<project>.conf`
  per project, and `{{home}}\data\nginx\php-upstreams.conf`. Any other file
  dropped into `vhosts\` is deleted on the next sync. **Editing these files has no
  lasting effect**, and there's no per-project include for custom rules.
- Limits: request bodies up to 64 MB, PHP (FastCGI) timeouts of 300 seconds.
- Error log: `{{home}}\data\nginx\logs\error.log`. The output of every service
  also shows on the app's **Logs** page (in memory only, the last 1000 lines).
- Each PHP version is an `upstream` of 4 workers (`least_conn`), so one slow
  request doesn't block others on the same version.

## PHP

- Versions live in `{{home}}\bin\php\<version>\` (official php.net NTS x64
  builds). The active version is chosen on the **Switch** page, and
  `{{home}}\current\php` points to it.
- **Per-project versions:** a project can pin its own version (the PHP button on
  its card). Every pinned version runs as its own service (`php-<version>`) on
  its own ports, at the same time as the others. So project A can run PHP 7.4
  while project B runs 8.3. A pin to a version that's no longer installed falls
  back to the active version.
- **On the command line:**
  - The **terminal button on a project card** opens a `cmd` window with that
    project's PHP (and Node) version first on `PATH`. That's the reliable way to
    run `php artisan`, `composer` and `npm` with the right versions.
  - **PHP everywhere** (Switch page, off by default) puts `{{home}}\current\php`
    first on the user `PATH`, so plain `php` anywhere means Rezure's active PHP.
    Run `where php` to see which PHP a shell actually gets. If it's
    `C:\laragon\...`, PHP everywhere is off.
- **php.ini, three layers:**
  1. **Generated**, rewritten on every PHP start:
     `{{home}}\data\php\ini\<version>-<hash>\php.ini`. This is what the web uses.
     **Don't edit it.**
  2. **Command line:** `{{home}}\bin\php\<version>\php.ini`. Used by `php.exe`,
     never by the web.
  3. **Your overrides:** `{{home}}\etc\php\conf.d\*.ini`, loaded after both of
     the above for the web **and** the command line. **This is where PHP settings
     belong**, e.g. `{{home}}\etc\php\conf.d\50-memory.ini` containing
     `memory_limit = 512M`. Restart PHP from the Services page afterwards.
- Defaults Rezure sets: `memory_limit 256M`, `upload_max_filesize` and
  `post_max_size 64M`, `max_execution_time 300`, `date.timezone UTC`.
- **Extensions:** on by default (when the build has them): curl, fileinfo, gd,
  intl, mbstring, mysqli, openssl, pdo_mysql, pdo_sqlite, sqlite3, zip, and redis
  when installed. OPcache is off. Others are turned on or off on the **PHP
  Extensions** page. Rezure keeps that choice in
  `{{home}}\data\php\<version>\extensions.json` and writes the `extension=` lines
  itself, so don't add them by hand.
- **Outgoing HTTPS:** PHP verifies certificates against
  `{{home}}\etc\cacert.pem` (`curl.cainfo` / `openssl.cafile`). A
  `cURL error 60` means that file is missing or stale. The Switch page can
  download a fresh one.

## Database

- One MySQL-family server at a time, service id `mariadb`, running either
  MariaDB or MySQL depending on the **active profile** (Databases page).
  PostgreSQL is a separate service on its own port and can run beside it — see
  [PostgreSQL](#postgresql).
- The default profile is Rezure's own MariaDB, with its data in
  `{{home}}\data\mariadb\data` on port 3306.
- Other profiles can adopt an existing data folder, such as Laragon's or XAMPP's.
  Such a profile runs that folder with a matching engine and version, and its own
  port. Switching profiles swaps which data is served. It never merges or copies
  data.
- **Login:** host `127.0.0.1`. Rezure's own data folder has user `root` with an
  **empty password**. An adopted data folder keeps its own users and passwords
  (Laragon's and XAMPP's default is also `root` with an empty password).
  `{{home}}\AGENTS.md` says which profile is active.
- Manage profiles from the Databases page, not by editing `profiles.json`: the app
  keeps it in memory and saves over it.
- SQL exports go to `{{home}}\dumps\`.

## SQL Server

Only for projects whose database is SQL Server. Two parts, installed from the app
on request, and both are real Windows installs (Settings > Apps), not files under
`{{home}}`:

- **Microsoft ODBC Driver for SQL Server** (17 or 18) plus PHP's `pdo_sqlsrv` /
  `sqlsrv` extensions (PHP Extensions page). PHP loads `pdo_sqlsrv` fine without
  the ODBC driver; it only fails on connect with "This extension requires the
  Microsoft ODBC Driver for SQL Server".
- **SQL Server Express LocalDB**, optional: a local SQL Server shown on the
  Services page (service id `sqlserver`). Rezure creates one instance, `Rezure`,
  reached as `(localdb)\Rezure` over a named pipe. There is no TCP port.
- **Login to LocalDB:** Windows Authentication. Leave `DB_USERNAME` and
  `DB_PASSWORD` empty. The instance belongs to the Windows user running Rezure.
- **Over TCP** (for tools that can't open a pipe): `127.0.0.1,14330`, SQL login
  `rezure` with an empty password, encryption off (LocalDB offers no TLS through
  the relay). Only while the Rezure app is running.
- Databases Rezure creates or restores on LocalDB keep their files in
  `{{home}}\data\mssql\`. Exports from LocalDB are `.bak` backups in
  `{{home}}\dumps\`.
- A SQL Server on another machine is a connection on the Databases page; Rezure
  lists its databases but doesn't start, stop or back it up.
- **ODBC Driver 18 encrypts by default** and refuses a self-signed certificate,
  which most development and office servers have. In Laravel, uncomment
  `'trust_server_certificate'` in `config/database.php` **and** set
  `DB_TRUST_SERVER_CERTIFICATE=true` (the env var alone does nothing, because the
  line ships commented out). LocalDB doesn't need this.

## PostgreSQL

Optional, installed from the app (Services page, or Switch > Install version).
Service id `postgres`, on `127.0.0.1:5432`. Portable builds from EDB under
`{{home}}\bin\postgres\<version>\`, nothing installed into Windows.

- **Login:** superusers `postgres` and `root`, any password (authentication is
  `trust`, and the server only listens on `127.0.0.1`). `root` exists because
  it's Laravel's default `DB_USERNAME`.
- **Versions:** the active one is picked on the **Switch** page. Each major
  version keeps its own databases in `{{home}}\data\postgres\<major>\`;
  switching from 17 to 18 starts on 18's data, it doesn't upgrade 17's. Move
  data across with an export and an import.
- **Databases page:** a connection called **PostgreSQL** appears by itself.
  Exports are plain `.sql` files from `pg_dump` (`--no-owner`), imports run a
  `.sql` file with `psql`, stopping at the first error.
- **PHP:** `pdo_pgsql` / `pgsql` ship with every PHP build but are **off by
  default** — turn `pdo_pgsql` on in PHP Extensions, or Laravel fails with
  "could not find driver".
- A PostgreSQL on another machine is a connection on the Databases page.

## Mail (Mailpit)

Mailpit catches every message sent to it. Nothing reaches a real inbox. Messages
show at `http://127.0.0.1:8025`. It accepts any SMTP username and password.

## Laravel `.env` for a Rezure project

```dotenv
APP_URL=http://<project>.test

DB_CONNECTION=mysql        # or mariadb (Laravel 11+)
DB_HOST=127.0.0.1
DB_PORT=3306               # the active profile's port
DB_DATABASE=<project>
DB_USERNAME=root
DB_PASSWORD=               # empty on Rezure's own data folder

MAIL_MAILER=smtp
MAIL_HOST=127.0.0.1
MAIL_PORT=1025
```

`MAIL_HOST=mailpit` (Laravel Sail's default) doesn't resolve outside Docker. Use
`127.0.0.1`.

For a project on SQL Server LocalDB instead:

```dotenv
DB_CONNECTION=sqlsrv
DB_HOST='(localdb)\Rezure'
DB_PORT=                   # must be empty: Laravel appends any port, even its 1433 default
DB_DATABASE=<project>
DB_USERNAME=               # empty = Windows Authentication
DB_PASSWORD=
```

For a project on Rezure's PostgreSQL:

```dotenv
DB_CONNECTION=pgsql
DB_HOST=127.0.0.1
DB_PORT=5432
DB_DATABASE=<project>
DB_USERNAME=root           # or postgres
DB_PASSWORD=               # anything: authentication is trust
```

**Using more than one of these in one project.** MySQL/MariaDB, SQL Server LocalDB
and PostgreSQL can all run at the same time (only MySQL and MariaDB take turns,
by profile). But Laravel's `config/database.php` reads the same `DB_HOST`,
`DB_PORT`, `DB_USERNAME`, `DB_PASSWORD` and `DB_URL` for every connection, so
changing `DB_*` moves them all. Keep `DB_*` for the default connection and give
each other connection its own variables in `config/database.php` (for example
`PG_HOST`/`PG_PORT`/`PG_DATABASE`, or `MSSQL_HOST='(localdb)\Rezure'` with
`MSSQL_PORT` empty), then use `DB::connection('pgsql')` or
`protected $connection = 'pgsql';` on models and migrations. Each server also
needs its PDO driver: `pdo_mysql` is on by default, `pdo_sqlsrv` is installed from
PHP Extensions, `pdo_pgsql` has to be turned on there.

## Node and Composer

- Node versions live in `{{home}}\bin\node\<version>\`. A project can pin its own
  Node version. It applies to the terminal the project card opens, nowhere else.
- Composer lives in `{{home}}\bin\composer\<version>\composer.phar`. **Rezure
  doesn't put a `composer` command on `PATH`.** Either use a globally installed
  Composer (it runs with whichever `php` comes first on `PATH`), or run
  `php {{home}}\bin\composer\<version>\composer.phar <command>`.

## Sharing a project

The **Share** button on a project card opens a temporary public
`https://*.trycloudflare.com` URL through `cloudflared`. It forwards to the
project on port 80 and stops when sharing is stopped or the app quits. Links the
app builds from `APP_URL` still point at `.test`, so they won't work for the
person you shared it with.

## Coexisting with Laragon and XAMPP

- Rezure and Laragon can both be installed. Only one of them can hold ports 80
  and 3306 at a time.
- Rezure's PHP workers start at port 9100 on purpose, so they don't collide with
  the 9000 that Laragon's and XAMPP's PHP usually use.
- A database profile can serve Laragon's MySQL data folder with Laragon's own
  binary. Rezure refuses to switch to it while Laragon's server is still running
  against that folder.
- Laragon's projects (`C:\laragon\www`) aren't Rezure's projects. Rezure only
  serves `{{home}}\www\` and the folders linked into it.

## Rules for AI agents

**Do**
- Treat Rezure as this machine's dev server for projects in `{{home}}\www\` or
  listed in `{{home}}\AGENTS.md`.
- Put PHP settings in `{{home}}\etc\php\conf.d\*.ini`.
- Use `http://<project>.test` for URLs and `127.0.0.1` for the database (`root`
  with an empty password, unless `{{home}}\AGENTS.md` says the active profile is an
  adopted data folder).
- Check `where php` and `php -v` before assuming which PHP runs a command.
- Ask the user to start, stop or restart services, switch versions, sync hosts,
  or toggle extensions **in the Rezure app**.

**Don't**
- Don't look for Laragon's or XAMPP's config to explain how a Rezure project is
  served.
- Don't edit anything under `{{home}}\data\` (generated), the hosts file's Rezure
  block, `{{home}}\current\`, or the JSON files in `{{home}}\etc\` while the app is
  running.
- Don't start, stop or kill `nginx.exe`, `php-cgi.exe`, `mysqld.exe`,
  `postgres.exe`, `mailpit.exe` or LocalDB's `sqlservr.exe` yourself. Don't run
  `pg_ctl` or `initdb` against `{{home}}\data\postgres\`. Don't run `SqlLocalDB` to
  delete or recreate the `Rezure` instance.
- Don't install or uninstall the ODBC Driver or LocalDB yourself. The app does
  both, after the user accepts Microsoft's license.
- Don't expect HTTPS on `.test` domains. There is none.

## Making agents find this

Rezure keeps `{{home}}\AGENTS.md` (current state), `{{home}}\CLAUDE.md` and this
guide up to date while it runs. Claude Code reads `CLAUDE.md` from every parent
folder, so it picks this up automatically in projects under `{{home}}\www\`.

A linked project lives outside `{{home}}`, so nothing points agents here from
inside it. To fix that for Claude Code in every project, add this line to
`%USERPROFILE%\.claude\CLAUDE.md` (or to a single project's `CLAUDE.md`):

```
@{{home}}\AGENTS.md
```

Other agents that read an `AGENTS.md` can be pointed here the same way from their
own instruction files.
