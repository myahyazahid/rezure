# Projects

A project is a folder Rezure serves at its own `.test` domain. There are two ways one gets
there: put it in the `www` folder, or point Rezure at it wherever it already lives.

---

## The two kinds

| | Scanned | Linked |
|---|---|---|
| Where it lives | `C:\rezure\www\<name>` | anywhere else |
| How it's added | drop the folder in | **Add folder** on the Projects page |
| How it's removed | move the folder out | **Remove** (the folder is left alone) |
| Recorded in | nothing — the folder *is* the record | `C:\rezure\etc\links.json` |

Both behave identically once listed: same virtual host generation, same hosts-file entry, same
Open / folder / terminal buttons, same "Used by" matching on the Databases page.

That's not a coincidence — `services::projects::scan_projects` is the single function that
answers "what projects exist", and all five of those consumers read it. Linked projects work
everywhere by virtue of appearing there.

---

## Linking a folder

**Add folder** on the Projects page opens a folder picker, then shows what you're about to get
before anything is saved:

- the **name**, from the folder (editable),
- the detected **framework** — with **Change** next to it if detection got it wrong (see
  [Frameworks](#frameworks)),
- the **document root** nginx will actually serve — for Laravel and CodeIgniter 4 that's
  `public/`, not the folder you picked,
- the **domain** (editable).

If the folder you picked is the `public/` (or `web/`, `webroot/`) folder *of* a project — the
usual workaround when an older Rezure served a project from the wrong folder — the dialog says so
and offers to add the project folder instead. Linking `public/` directly does serve the site, but
the project's folder and terminal buttons then open `public/`, and its name and domain are taken
by the wrong folder.

Nothing is copied, moved, or written inside the folder. Rezure records the path.

### Domains

The default is `<folder-name>.test`. If that's already taken by another project, the next free
number is used (`api-2.test`) and the dialog says why it isn't the obvious one. Comparison is
case-insensitive, since `API.test` and `api.test` are one domain to nginx and the hosts file.

You can set any free domain you like at link time.

### What gets refused

| Path | Why |
|---|---|
| A drive root (`C:\`) | Serving a whole drive over HTTP exposes the machine |
| `C:\Windows`, `C:\Program Files` | Same, and never legitimate |
| Anything inside `www` | Already listed by the scan — linking it too would produce two virtual hosts for one folder |
| A folder already linked | It's one project, not two |
| Anything that isn't a folder | — |

The first two are refused rather than warned about: there is no correct version of them.

A folder with **no recognizable framework** is *not* refused. You picked it deliberately; Rezure
notes that it found no framework markers and serves it from the folder itself.

---

## Frameworks

Every project is served as some framework, and the framework decides **how nginx serves it**:

- A PHP framework decides **which folder** is served. Laravel and CodeIgniter 4 keep their front
  controller in `public/index.php`, so they're served from `public/`; WordPress and plain PHP are
  served from the project folder. Get it wrong and the site 404s even though the code is fine.
- A JavaScript or Node framework (Vue, React, Svelte, Angular, Astro, Next.js, Nuxt, Node) is
  served by its **dev server** instead — see [Dev servers](#dev-servers).

### How it's detected

Rezure reads marker files — `artisan`, `spark`, `wp-config.php`, packages in `composer.json` and
`package.json` — and weighs every framework that matches. The **most specific wins**, not the one
that happens to be checked first:

| Priority | Kind | Examples |
|---|---|---|
| 90 | PHP frameworks with their own marker file | Laravel, CodeIgniter 3/4, WordPress, Bedrock, Drupal, Joomla |
| 80 | PHP frameworks recognised through Composer | Symfony, Yii 2, CakePHP, Slim, Laminas |
| 55 / 50 | JavaScript frameworks | Next.js, Nuxt, Astro / Vue, React, Svelte, Angular |
| 30 | Any PHP app with `public/index.php` and no `index.php` of its own | |
| 20 / 15 / 10 | Plain PHP / Node / static HTML | |

So a CodeIgniter 4 app that keeps a `package.json` to build its CSS is CodeIgniter 4, not Node,
and a Laravel app on Inertia is Laravel, not Vue. Nothing is run to find out — only files are read.

The full list, and the format, are in [frameworks.md](frameworks.md).

### Correcting it

Click the framework badge on a project (list or grid view). Pick another framework, or
**Custom folder…** to serve a subfolder of your choosing — which is also how a JavaScript
project's built `dist/` is served as plain files instead of through its dev server. **Auto** goes
back to detection. The change is live immediately: the vhost is rewritten and nginx reloaded.

This works for scanned and linked projects alike. A badge with a dot was set by hand. Choices are
kept in `C:\rezure\etc\project-frameworks.json`, by project id:

- a project in `www` is identified by its folder name, so **renaming the folder drops its choice**;
- removing a linked project drops its choice with it.

### Adding a framework Rezure doesn't know

Describe it in `C:\rezure\etc\frameworks.json` and click **Reload frameworks.json** in the
framework dialog (or restart the app). No release needed — see
[frameworks.md](frameworks.md#your-own-definitions). If something in that file can't be used, the
Projects page lists what and why; the rest of the file still applies.

---

## Dev servers

A Vite, Astro, Next.js or Nuxt project — or a Node app like 9router — isn't a folder of files a
web server can hand out: it only works while its own process runs. For those, nginx passes every
request for the project's domain to that process's port, and Rezure can run the process for you.

### Opening one

Click **Open**. If the dev server isn't running, Rezure starts it, waits until it answers, and
only then opens the browser — so you land on the app, not an error page. The dot on the Open
button is green while it runs. If it fails to start, the browser isn't opened; the message under
the header says why, with **View log** for the rest.

### The Dev servers dialog

**Dev servers**, next to Sync hosts file, lists every project served this way:

| Setting | What it does |
|---|---|
| Port | Where nginx sends the domain. Rezure picks one from 4400–4499 the first time and keeps it, so `astro-web.test` always reaches the same port. Change it for an app that listens on a port of its own (9router: 20128). |
| Tell it the port with | How the dev server is made to use that port — see below. The default comes from the framework. |
| Script | The `package.json` script that runs it. Default: the framework's (`dev` for Vite, Astro, Next, Nuxt; `start` for Angular and Node), else `dev`, else `start`. |
| Open starts it | Off: Open just opens the browser, and you start the server yourself. |

Port and script can only be changed while the dev server is stopped. The row also shows the
exact command, a **Log** of its output, and what's missing when it can't start yet: `node_modules`
(with an **Install** button that runs `npm install` — or pnpm/yarn/bun, going by the lockfile), a
Node.js version that isn't installed, or a script `package.json` doesn't have.

How the port is passed:

| Option | Added to the command | For |
|---|---|---|
| `--port` + `--strictPort` | `-- --port 4400 --strictPort` | Vite (Vue, React, Svelte). Without `--strictPort` Vite quietly moves to another port when its own is taken. |
| `--port` | `-- --port 4400` | Next.js, Nuxt, Astro, `ng serve` |
| `PORT` variable only | nothing | Create React App, and Node apps that read `process.env.PORT` |
| The app's own port | nothing | Apps with a port of their own — enter that port above |

`PORT` is set in every case. If the server comes up on a different port than the one set, the
error names the port it actually took.

### What Rezure does and doesn't do

- Dev servers are started one at a time, by you (Open or Start) — never by **Start all** on the
  Services page, where they aren't listed. Each can take hundreds of MB.
- They run on the project's own Node.js version when it pins one.
- **Every dev server is stopped when Rezure closes.** One left over from a crash is cleaned up the
  next time it's started.
- A dev server that exits on its own isn't restarted — a compile error would only crash it again.
  Its log says what happened.
- Settings are kept in `C:\rezure\etc\dev-servers.json`, by project id. Removing a linked project
  drops its entry.

### Sharing

**Share** works the same as for any project: the tunnel goes through nginx to the dev server.

---

## When a linked folder goes missing

A linked project whose folder is gone — moved, deleted, or on a drive that isn't plugged in —
stays in the list, marked, rather than quietly disappearing. Silently dropping it would look
like Rezure lost the project, and the unplugged-drive case fixes itself.

While missing it gets **no virtual host** (nginx can't serve a root that isn't there), but it
**keeps its hosts-file entry** — removing it every time an external drive is unplugged would
mean going through an admin prompt again to get it back.

---

## Removing

**Remove** on a linked project deletes nothing. Rezure stops serving it and drops its virtual
host; the folder and everything in it stays exactly where it is, and you can add it again any
time.

Scanned projects have no Remove button — move the folder out of `www` instead.

---

## Checking a project's requirements

The stethoscope button on a project runs one check: every `ext-*` in that project's
`composer.json`, compared against what the active PHP actually loads.

It exists because that gap fails silently. Composer refuses to install without an extension,
but a project installed elsewhere — cloned from a repo, copied from another machine — gets no
such warning, and a Laravel app missing `ext-intl` answers a browser with a blank 500 while the
real reason sits in `storage/logs/laravel.log`.

The answer is about the PHP that serves the site: the active version, running with the same
generated ini and the same settings folder the web server uses. Requirements that only
`require-dev` asks for are listed but not counted as missing — a served site doesn't break
without them.

Nothing runs on its own. The check spawns `php -m`, so it happens when you ask for it, not once
per project every time the page opens.

Missing something? For an extension Rezure can install — `redis` today — the check offers an
**Install** button next to it, and re-runs itself afterwards. For anything else, the fix is a line
in your settings folder: see [Configuring PHP](php-versions.md#configuring-php).

An install reaches new requests only after the PHP service restarts; the copy already serving
requests read its configuration when it started.

---

## How it works under the hood

### Identity

Scanned projects are identified by their folder name. Linked ones need more, because two
folders called `api` in different places must not collide — `services::launcher` resolves an id
back to a folder in order to open it, and picking the wrong one would open the wrong project.

A linked id is `<slug>-<6 hex of the path>`, e.g. `ordo-a1b2c3`. The hash is derived from the
path rather than random, so re-linking the same folder produces the same id and its recorded
history (`opened N×`, last opened) comes back with it instead of starting over.

### Where the registry lives, and why not SQLite

Linked folders are stored as JSON, not in Rezure's SQLite database, for a structural reason:
`scan_projects` is a plain synchronous function called from five places in `services/`, while
the SQLite connection lives in Tauri's managed state and can't be reached from there. Putting
the registry in JSON behind a `OnceLock` — the same shape as `config::profiles` — avoids
threading a database handle through all five.

It also draws a cleaner line:

| Store | Holds |
|---|---|
| `links.json` | *which* projects exist — configuration, declared by the user |
| SQLite `projects` | history *about* projects — derived, and disposable |

### Framework detection

`services::frameworks` holds a registry of definitions — built-in ones from
`src-tauri/resources/frameworks.json`, with the user's `etc\frameworks.json` merged over them —
and `services::projects::scan_projects` resolves every project against it, applying any choice
from `etc\project-frameworks.json` on top. Because that happens inside the one scan everything
reads, the corrected docroot reaches vhost generation, the project list and `AGENTS.md` without
any of them knowing about overrides. `ProjectInfo` carries the result: `framework` (effective id),
`detected_framework`, `framework_overridden`, the label as `stack`, and the absolute `docroot`.

Definitions are data, but how nginx serves a project isn't: a definition only names a serve
profile (`php` or `proxy`), and the template behind it lives in `services::vhosts`. All vhosts
share one `nginx.conf`, so a definition able to add directives could take every site down.

### Dev servers

A `proxy` project's vhost passes everything to an nginx `upstream` holding `127.0.0.1:<port>` and,
as a backup, `[::1]:<port>` — Vite listens on `localhost`, which on most Windows machines is IPv6
only. `Upgrade`/`Connection` are passed on for hot reload, `Host` stays the project's domain, and
buffering is off so streamed responses arrive as they're written. nginx's own 502 (nothing
listening) is replaced by a page saying the dev server isn't running. `php` vhosts answer 404 for
every dotfile (`.env`, `.git`) except `.well-known`; `proxy` ones don't, since Vite serves its own
modules from `/node_modules/.vite/`.

The port comes from `etc\dev-servers.json` (`config::dev_servers`). The vhost sync
(`vhosts::sync_vhosts`) is what picks one for a project that has none, through
`dev_servers::assign_ports`; the scan only reads it into `ProjectInfo::dev_port`.

Next's dev endpoints (`/_next/`, `/__nextjs`) get their own `location`, identical but for
`Origin`, which is dropped when it matches the project's domain — Next 16's
`blockCrossSiteDEV` 403s every other origin than `localhost`, the hot-reload websocket included.

Each `proxy` project is registered with `ServiceManager` as a service `dev-<project id>`
(`ServiceManager::sync_dev_servers`, the same shape as the PHP pool), built from a `DevSpec`:
port, port mode, script, package manager, Node.js pin. It runs as `cmd.exe /d /s /c "npm run dev
-- --port 4400 --strictPort"` in the project folder — a command assembled only from a fixed
package manager, a script name restricted to letters, digits and `:_.-`, and a number. Before
spawning, `dev_servers::outside_servers` looks for a copy started elsewhere: a node/bun/deno
process whose working folder is the project's, not descended from this Rezure, with something
under it listening; the dialog offers to stop it (`dev_servers::stop_outside`, which kills from
the topmost runtime down, never the shell). A failed start is summarised from the last 40 lines
of output, stack frames dropped, with a fix for the familiar failures. A start
waits until the port answers; a stop kills the whole process tree (`npm` runs the server as a
grandchild). Dev servers are left off the Services page and out of Start all
(`service_visibility`), carry no telemetry (their ids are project names), and are stopped with
everything else on exit.

Choices live in JSON rather than SQLite for the same structural reason as `links.json`, and one
more: SQLite here is derived, disposable history, while a framework choice is a declaration that
must survive the database being reset.

---

## Troubleshooting

**A linked project's domain doesn't resolve in the browser**
It needs a hosts-file entry. Use **Sync hosts file** — it prompts for admin rights once and
writes every project's domain at the same time.

**The site 404s (or lists a directory) although the project works elsewhere**
Check where it's served from: hover the framework badge. A front-controller framework served from
the project folder instead of `public/` answers every URL with a 404. Click the badge and pick the
right framework, or **Custom folder…** if it uses its own layout.

**"Some framework settings aren't being used"**
An entry in `etc\frameworks.json` was skipped (the message says why), or a project is set to a
framework that's no longer defined — it's served as detected meanwhile. Fix the file, then click
**Reload** next to the message.

**A JavaScript project shows "This project's dev server isn't running"**
That's nginx saying nothing answers on the project's port. Click **Open**, or **Start** in Dev
servers. If you start the server yourself, make it listen on the port Dev servers shows.

**"Blocked request. This host is not allowed" from Vite**
Rezure tells Vite to accept the project's domain through `__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS`.
A Vite version too old for that needs `server: { allowedHosts: ['.test'] }` in `vite.config`.

**Hot reload doesn't update the page**
Open the site through its `.test` domain after the dev server is up, and check the browser console
for a failed websocket. A `server.hmr` setting in `vite.config` that names another port or host
overrides what nginx forwards.

**"it's listening on :3001 instead of :4400"**
The dev server ignored the port it was given — typically a script that wraps it (`concurrently`,
a custom `server.js`). Either set Dev servers' port to the one it uses (and "The app's own port"),
or make the script pass `--port` on.

**"It's already running outside Rezure"**
You started the same dev server yourself — `npm run dev` in a terminal, or an IDE's run button.
Astro and Next refuse to run twice in one folder, and two copies would fight over ports anyway.
Click **Stop it and start here** in Dev servers: Rezure stops that copy's `node` (the terminal
it was typed in stays open) and starts its own on the project's port. Rezure only treats a
process as that copy when it runs in the project's folder *and* listens on a port, so a build
watcher or test runner there is left alone.

**A Next.js page stays on its loading screen**
Next 16 refuses its own dev endpoints (`/_next/*`, including the hot-reload websocket) to any
origin but `localhost` unless it's in `allowedDevOrigins`. Rezure's vhost drops the `Origin`
header on those paths when it's the project's own domain, so nothing needs adding to
`next.config`. A request from any other site is still refused.

**A dev server won't start: "its dependencies aren't installed"**
Click **Install** in Dev servers, or run the install from the project's terminal.

**"it's already inside your www folder"**
The folder is picked up by the scan automatically; there's nothing to add.

**The domain got a `-2` suffix**
Another project already claimed the plain name. Both are listed — check which, and rename one
if the numbered domain isn't what you want.

**A linked project shows "Folder not found"**
The path in `links.json` no longer resolves. If the folder moved, remove the project and add it
at its new location; if it's on a drive that isn't connected, plug it in and the project comes
back on its own.
