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

Every project is served as some framework, and the framework decides one thing: **which folder
nginx serves**. Laravel and CodeIgniter 4 keep their front controller in `public/index.php`, so
they're served from `public/`; WordPress and plain PHP are served from the project folder. Get the
framework wrong and the site 404s even though the code is fine.

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
**Custom folder…** to serve a subfolder of your choosing. **Auto** goes back to detection. The
change is live immediately: the vhost is rewritten and nginx reloaded.

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
profile (today only `php`), and the template behind it lives in `services::vhosts`. All vhosts
share one `nginx.conf`, so a definition able to add directives could take every site down.

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

**"it's already inside your www folder"**
The folder is picked up by the scan automatically; there's nothing to add.

**The domain got a `-2` suffix**
Another project already claimed the plain name. Both are listed — check which, and rename one
if the numbered domain isn't what you want.

**A linked project shows "Folder not found"**
The path in `links.json` no longer resolves. If the folder moved, remove the project and add it
at its new location; if it's on a drive that isn't connected, plug it in and the project comes
back on its own.
