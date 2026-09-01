# PHP versions

Rezure can run any number of PHP versions side by side and switch which one nginx and
Composer use. There are **two ways to get a version in**, and they meet on the filesystem
rather than in a registry — so neither needs a registration step, and deleting a folder is a
complete uninstall.

---

## Where versions live

| Root | What's in it | Marked as |
|---|---|---|
| `C:\rezure\bin\php\<version>\` | Versions Rezure downloaded and checksum-verified | managed |
| `C:\rezure\custom\php\<anything>\` | Versions you dropped in yourself | added by hand |

Both roots are scanned on every listing (`binaries::discover`), so a folder appearing in
either **is** an installed version. The drop-in root sits next to `~\rezure\www` and
`~\rezure\dumps` deliberately: it's meant to be opened in Explorer and dropped into, the way
Laragon's `bin\` is.

If the same version exists in both roots, the managed copy wins — that's the one Rezure
verified.

---

## Option 1 — Install from the app

**Switch → Install version** lists what php.net currently publishes for Windows, with its
release date and archive size. Pick one and Rezure downloads it, verifies its SHA-256, and
extracts it into the managed root.

The list is **not** a hard-coded table. It's read at runtime from php.net's own release
index:

```
https://downloads.php.net/~windows/releases/releases.json
```

That's the same file the official download page is built from, and it already carries the
checksum, size and release date for the current release of every supported branch. Hand-
maintaining that list would mean it silently rots the moment php.net ships a patch release —
and every entry would need a real SHA-256 typed in by hand. Reading it live keeps the
installer current on its own *and* keeps the checksum verification a pinned entry would get.

Rezure picks the **non-thread-safe x64** build of each branch (`nts-…-x64`). That's the right
one here: PHP runs as `php-cgi.exe` behind nginx, never inside a threaded SAPI. The compiler
tag in the key changes between branches (`vc15`, `vs16`, `vs17`), so the build is matched by
shape rather than looked up by name.

The response is cached for the life of the process — the dialog gets opened far more often
than php.net publishes.

### What it can't offer

php.net's index only lists **the newest release of each branch**. There's no 8.3.12 in it,
only whatever 8.3.x is current. For an older patch release, use option 2.

---

## Option 2 — Drop one in, Laragon-style

Downloaded a build yourself? Two ways to register it:

**Unpack it into the drop-in folder.** Open `C:\rezure\custom\php\` (the Switch page
links to it) and extract the zip there. Any folder holding a `php.exe` counts — the folder
name doesn't matter:

```
C:\rezure\custom\php\
├── php-8.4.25-nts-Win32-vs17-x64\php.exe   ← reads as 8.4.25
└── 8.0.30\php.exe                          ← reads as 8.0.30
```

The version is taken from the first `1.2.3`-shaped token in the folder name. A folder one
level deep also works, so a zip extracted *without* "extract here" (leaving its own folder in
the middle) is still found.

**Or use "Add from folder…"** in the Install dialog. Point it at the folder you unpacked and
Rezure copies it into the drop-in root for you. This path asks the binary itself what version
it is (`php.exe -r "echo PHP_VERSION;"`) rather than trusting the folder name — a hand-renamed
folder would otherwise install under the wrong number and mislabel every switch after it.

It copies rather than referencing the original: there's no settings persistence yet (Phase 4),
so a list of external paths would have nowhere to live across restarts. Your download is left
untouched.

> **Dropped-in builds are not checksum-verified.** Rezure never saw where they came from. The
> Switch page counts them separately so it's clear which versions carry that guarantee and
> which don't. Get your builds from [windows.php.net](https://windows.php.net/download/).

---

## Switching

The Switch page's PHP dropdown lists **only installed versions** — installing is the dialog's
job.

Picking one **reloads PHP straight away**: if the service is running, Rezure restarts it onto
the new binary, so there's no manual restart step. `services::process` resolves the PHP
binary at spawn time rather than caching a path, which is what makes the restart land on the
new version at all.

Only PHP is restarted. nginx reaches it over `127.0.0.1:9000` per request and reconnects on
its own once the new process has rebound the port, so bouncing nginx too would drop live
requests for nothing.

If PHP wasn't running, nothing is restarted — the choice simply applies the next time it
starts, and the page says so.

If the restart *fails* (the port got taken in between, say), the switch still stands and the
page reports the failure separately: the active version really did change, and PHP is now
down. Reporting it as a failed switch would leave the UI showing the old version as active
while the backend had already moved on.

### Scope: what the switch actually affects

By default the active version reaches two places:

| | Follows the switch? |
|---|---|
| `php-cgi` behind nginx — your `.test` sites | yes |
| Composer, when scaffolding a Laravel project | yes |
| `php` in your own terminals | only with the PATH link below |

---

## Making it system-wide (optional)

**Switch → "Use Rezure's PHP everywhere"** puts the active version on your user PATH, so `php`
resolves to it in every terminal.

### How it works, and why not the obvious way

Laragon switches by writing the *versioned* folder into PATH
(`C:\laragon\bin\php\php-8.5.8-…`) and rewriting that entry on every switch. Rezure doesn't:
rewriting PATH repeatedly is how PATH gets corrupted, already-open terminals never see the
change, and two tools doing it end up fighting over who wrote last.

Instead Rezure adds **one stable entry, once**:

```
C:\rezure\current\php   →  junction  →  <active version's folder>
```

Switching re-points the junction. PATH is never touched again. And because the PATH string
doesn't change, **terminals you already have open pick up the new version too** — they resolve
`php` through the same directory, whose target moved underneath them.

Two different moments, easy to confuse:

| Action | Reaches already-open terminals? |
|---|---|
| **Enabling** — adds the entry to PATH | no — an open shell holds the environment it launched with, so open a new one |
| **Switching versions** while enabled | yes — the entry is already in its PATH, only the target moved |

A *junction*, specifically, not a symbolic link: directory junctions can be created without
administrator rights, symlinks can't. (That's why nvm-windows, which uses a symlink, ships an
`elevate.cmd`.) No UAC prompt, ever.

### It takes over from Laragon or XAMPP — deliberately

If another tool already puts `php` on your PATH, the card names it before you enable anything.
Rezure inserts its entry **first**, so it wins. That's the point of the feature, but it means
`php` system-wide stops being Laragon's and becomes Rezure's. **Disable** removes the entry and
hands it straight back.

Turning it off restores your PATH **byte for byte** — including a trailing `;` or any empty
segment it happened to have. Rezure only ever adds and removes its own entry; it never
reformats the rest.

### Safety notes

- The user PATH is read and written straight through the registry, preserving its value kind.
  Reading it via `[Environment]::GetEnvironmentVariable(…, 'User')` expands `%VAR%` references,
  and writing that back turns a `REG_EXPAND_SZ` entry into a literal — the classic way PATH
  gets quietly mangled.
- `WM_SETTINGCHANGE` is broadcast after a write, so newly-launched apps see it without a
  sign-out.
- The junction is replaced with `[IO.Directory]::Delete(link, $false)`, which removes only the
  reparse point. `Remove-Item -Recurse` would follow the link and delete the real PHP install
  on the other side.
- Re-pointing works while PHP is running; the running process keeps its own binary and is
  unaffected.

---

The active choice is in-memory for now (Phase 4's settings persistence hasn't landed), so a
restart falls back to the newest installed version. It's also self-healing: if the active
version's folder disappears — you deleted it from the drop-in root — the next lookup notices
and re-picks rather than failing.

---

## Troubleshooting

**A folder I dropped in isn't showing up.** It needs a `php.exe` directly inside it, or inside
exactly one subfolder. Reopen the Switch page to rescan.

**Two folders, one version number.** Only one shows. The managed copy wins; otherwise the
first one found.

**"Add from folder…" says PHP x.y.z is already installed.** That version already exists in the
drop-in root. Delete the old folder first, or just switch to it.

**A version won't start.** The zip has to be a complete PHP build — `php-cgi.exe` lives beside
`php.exe` and is what actually runs behind nginx. A folder with only `php.exe` in it will be
listed but won't serve.

**`php -v` in my terminal doesn't match Rezure.** The PATH link is off, or another tool
(Laragon, XAMPP) sits ahead of it. Check the "Use Rezure's PHP everywhere" card — it lists what
else provides `php`. `where php` shows the winner.

**Which copy am I running?** The path is on each version in the Switch dropdown's tooltip, and
`services::php::print_installed` prints the full picture:

```
cargo test --lib services::php::tests::print_installed -- --ignored --nocapture
```
