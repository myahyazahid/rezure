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
| Recorded in | nothing — the folder *is* the record | `%APPDATA%\Rezure\links.json` |

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
- the detected **stack**,
- the **document root** nginx will actually serve — for Laravel that's `public/`, not the
  folder you picked,
- the **domain** (editable).

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

A folder with **no recognizable stack** is *not* refused. You picked it deliberately; Rezure
notes that it found no framework markers and serves it as static files.

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

Missing something? The fix is a line in your settings folder — see
[Configuring PHP](php-versions.md#configuring-php). An extension whose DLL isn't in the build at
all has to be installed first; Rezure doesn't ship PECL extensions such as `redis` yet.

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

### Stack detection

Marker files, in order: `artisan` → Laravel, `wp-config.php` → WordPress, then `package.json`
dependencies (Next.js, Nuxt, Vue, React, Svelte, else Node), then `composer.json`/`index.php` →
PHP, then `index.html` → Static, else Unknown.

The stack decides the document root: Laravel projects are served from `public/`, everything
else from the project root.

---

## Troubleshooting

**A linked project's domain doesn't resolve in the browser**
It needs a hosts-file entry. Use **Sync hosts file** — it prompts for admin rights once and
writes every project's domain at the same time.

**"it's already inside your www folder"**
The folder is picked up by the scan automatically; there's nothing to add.

**The domain got a `-2` suffix**
Another project already claimed the plain name. Both are listed — check which, and rename one
if the numbered domain isn't what you want.

**A linked project shows "Folder not found"**
The path in `links.json` no longer resolves. If the folder moved, remove the project and add it
at its new location; if it's on a drive that isn't connected, plug it in and the project comes
back on its own.
