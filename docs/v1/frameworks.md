# Frameworks

Rezure works out which framework a project is from its files, and the framework decides which
folder nginx serves. This page is the reference for the definitions behind that: what Rezure
ships with, the format, and the two ways to add one — for everyone, or for your machine only.

For what this looks like in the app (the badge, correcting a project), see
[projects.md](projects.md#frameworks).

---

## What a definition is

```json
{
  "id": "codeigniter4",
  "label": "CodeIgniter 4",
  "priority": 90,
  "detect": { "any": [{ "file": "spark" }, { "composer": "codeigniter4/framework" }] },
  "docroot": "public"
}
```

| Field | Required | Meaning |
|---|---|---|
| `id` | yes | Lowercase letters, digits and hyphens, at most 64. Stable: project choices and telemetry refer to it, so a built-in id never changes once released. `unknown` and `custom` are reserved. |
| `label` | yes | What the Projects page shows, at most 40 characters. |
| `priority` | yes | 0–100. When several definitions match, the highest wins; on a tie, the one listed first. |
| `detect` | yes | When a project is this framework — see [Detect rules](#detect-rules). |
| `docroot` | no | The subfolder nginx serves, relative to the project. Left out, the project folder itself. If a project doesn't have that subfolder, it's served from its own folder — so one definition can cover two layouts. |
| `category` | no | The heading the framework pickers list it under, at most 24 characters. Built-ins use `PHP`, `JavaScript` and `Generic`; headings are ordered by the highest priority in them. A definition of your own without one is listed first, under "Your frameworks". Detection never reads it. |
| `serve` | no | The serve profile. Only `php` exists (the default): files as they are, everything else through `index.php` over FastCGI. |

Unknown fields are refused rather than ignored, so a typo like `docRoot` is reported instead of
silently doing nothing.

### Detect rules

A rule is an object with exactly one key:

| Rule | Matches when |
|---|---|
| `{ "file": "artisan" }` | that file exists in the project (paths may be nested: `system/core/CodeIgniter.php`) |
| `{ "dir": "web/app" }` | that folder exists |
| `{ "composer": "laravel/framework" }` | `composer.json` lists the package in `require` or `require-dev` |
| `{ "npm": "next" }` | `package.json` lists the package in `dependencies` or `devDependencies` |
| `{ "any": [ … ] }` | at least one of the rules inside matches |
| `{ "all": [ … ] }` | every rule inside matches |
| `{ "not": { … } }` | the rule inside doesn't match |

Package names are compared case-insensitively. `any` and `all` need at least one rule, and rules
nest at most 8 deep. Detection only ever reads files — it runs for every project each time the
Projects page opens, so nothing is executed.

### Paths

`docroot`, `file` and `dir` must be relative to the project: no drive letter, no leading slash, no
`.` or `..`, no empty segments. `$` and `"` are refused too — the docroot ends up in a quoted
string in the nginx config, where `$` would be read as a variable. `/` and `\` both work, and a
trailing slash is ignored.

### Priorities

The priority is what keeps a project from being mistaken for something it merely contains. A
CodeIgniter 4 app with a `package.json` for its CSS matches both CodeIgniter 4 and Node; a Laravel
app on Inertia matches both Laravel and Vue. Pick a priority by the band your definition belongs
to:

| Band | For | Built-ins |
|---|---|---|
| 90 | Frameworks with a marker file of their own | Laravel, CodeIgniter 3, CodeIgniter 4, WordPress, Bedrock, Drupal, Joomla |
| 80 | Frameworks recognised through Composer alone | Symfony, Yii 2, CakePHP, Slim, Laminas |
| 55 | JavaScript meta-frameworks (above the libraries they depend on) | Next.js, Nuxt, Astro |
| 50 | JavaScript frameworks | Vue, React, Svelte, Angular |
| 30 | Any PHP app with `public/index.php` and no `index.php` of its own | PHP (public/) |
| 20 / 15 / 10 | Fallbacks | PHP, Node, Static |

A project nothing matches is **Unknown** and served from its own folder.

---

## Adding a framework for everyone

Built-in definitions live in [`src-tauri/resources/frameworks.json`](../../src-tauri/resources/frameworks.json),
embedded into the app at build time. A pull request adding one has two parts:

1. **The entry** in `frameworks.json`.
2. **A fixture**: a folder under
   [`src-tauri/tests/fixtures/frameworks/`](../../src-tauri/tests/fixtures/frameworks/) named after
   the id, holding the smallest set of files that identifies the framework — the marker files, a
   `composer.json` / `package.json` with the relevant package, and the docroot folder with an
   `index.php`. Use `<id>--<variant>` for more than one shape (`codeigniter4--with-sass`).

No test code is needed. `cargo test` checks that the built-in file is valid, that every fixture is
detected as the id in its folder name, and that every built-in id has at least one fixture. When a
new definition could steal projects from an existing one, add a fixture for that case too —
`laravel--with-vue` and `nextjs--with-react` are there for exactly that.

To see what a definition makes of real projects without linking them:

```powershell
$env:REZURE_DETECT_DIR='C:\path\to\your\projects'
cargo test --lib services::projects::tests::print_detection -- --ignored --nocapture
```

---

## Your own definitions

A framework that's internal to your company, or one Rezure doesn't ship yet, goes in
`C:\rezure\etc\frameworks.json` (`%REZURE_HOME%\etc\` if you moved Rezure's home). Same format,
wrapped in a `frameworks` list:

```json
{
  "frameworks": [
    {
      "id": "acme-cms",
      "label": "Acme CMS",
      "priority": 85,
      "detect": { "any": [{ "composer": "acme/cms-core" }, { "file": "acme.php" }] },
      "docroot": "htdocs"
    }
  ]
}
```

- A **new id** adds a framework. **A built-in id** (`laravel`, `codeigniter4`, …) replaces that
  definition entirely.
- The file is read when the app starts and when you click **Reload frameworks.json** in a
  project's framework dialog — not on every change, like the rest of Rezure's config.
- An entry that can't be used is skipped on its own; the others still apply. The Projects page
  lists what was skipped and why. A file that isn't valid JSON at all is ignored as a whole, and
  that's listed too.
- Your definitions appear in the framework pickers marked "yours".

---

## Serving a project some other way

A one-off layout doesn't need a definition: on the project's framework dialog, pick **Custom
folder…** and name the subfolder to serve. It's kept per project in
`C:\rezure\etc\project-frameworks.json`; don't edit that file by hand while the app is running.

What no definition can do is change the nginx rules themselves. Every project gets the same
`server` block, and all of them are loaded from one `nginx.conf`, so a definition able to add
directives could stop every site. Serve profiles beyond `php` — a static or single-page-app
profile, or a reverse proxy to a dev server — would be added in Rust, in `services::vhosts`, and
then become available to definitions by name.
