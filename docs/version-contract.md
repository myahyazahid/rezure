# Auto-update — what `rezureapp` expects from the version endpoint

This documents the client side of auto-update: what triggers a check, what shape the
response must have, and why. The canonical wire contract — request/response shapes,
auth, rate limits — lives outside this repo at `api-documentation/telemetry-api.md`'s
`GET /version/latest` section (shared with `laravel-api`, since both sides must agree on
it). If the two disagree, that doc wins; update this one to match.

**Backend status:** `GET /api/v1/version/latest` now returns the signed-manifest shape
below and the `releases` table has `signature`/`download_url` columns — a maintainer
attaches both when publishing a release from the dashboard's Releases page. A release
published without them is still visible to `/changelog` and the plain `version`/`notes`
fields here, it's just never offered as an auto-update (`platforms` comes back `{}`).

## Why this is stricter than the original roadmap sketch

`docs/v3/rezure-app-v3-phases-tasks.md`'s Fase 3.4 originally sketched a plain
`{version, download_url}` response. That was superseded during implementation: a
checksum or URL returned by the same server that also serves the download gives no
real integrity guarantee — if that server is compromised, both the file and its
"proof" are compromised together. Rezure installs run at elevated privilege, so the
endpoint must instead return a response `tauri-plugin-updater` can verify with an
**ed25519 signature**, checked against a public key baked into the app at build time
— independent of anything the server itself claims.

## Response shape (exact — this is Tauri's own updater manifest format)

`GET https://api.redscale.my.id/api/v1/version/latest`:

```json
{
  "version": "3.1.0",
  "notes": "Bug fixes and performance improvements.",
  "pub_date": "2026-09-14T12:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "<contents of the .sig file produced alongside the installer>",
      "url": "https://.../rezureapp_3.1.0_x64-setup.exe"
    }
  }
}
```

- `version` must be semver and strictly greater than the requesting client's current
  version for `tauri-plugin-updater`'s `check()` to treat it as an update. Rezure never
  passes `allowDowngrades`, so anything else is silently treated as "no update".
- `signature` is the base64 content of the `.sig` file the Tauri bundler produces next
  to the installer when `bundle.createUpdaterArtifacts` is on (`src-tauri/tauri.conf.json`)
  — produced by signing the release with the private half of the keypair from
  `tauri signer generate`. The private key and its password are a release-time secret;
  they never live in this repo.
- `windows-x86_64` is the only platform key that matters — Rezure is Windows-only.
- **Already on the latest version:** respond `204 No Content` (Tauri's own convention
  for "nothing to update to"), not `200` with a same-or-lower `version` — the plugin's
  behavior differs subtly between the two, and `204` is the unambiguous one.

## Release lines: 3.x, 4.x, ... side by side

Versions are strictly `MAJOR.MINOR.PATCH`, the same string in `tauri.conf.json`,
`Cargo.toml`, `package.json` and the dashboard's publish form: `3.0.1`, never `v3.0.1` or
`V.3.0.1`. The `v` prefix belongs on git tags only. The dashboard rejects anything else,
because the updater can't parse it.

- A **new major** (`4.0.0`) is a big release. **Minor/patch** releases (`3.0.1`, `3.1.0`)
  are fixes and small additions within a line.
- Each line lives on its own branch (`v1`, `v2`, `v3`, ...). A fix for an older line is
  made on that line's branch, tagged (`v3.0.2`), and built and signed from the tag.
- **The updater never crosses lines.** The backend reads the major off `current_version`
  and only answers with releases from that line, highest version first (not most recently
  published). A 3.x install gets 3.0.1, 3.1.0, ... and is never offered 4.0.0.

Because of that, a 3.x user would never hear about 4.0 through the updater. That's what
the upgrade notice is for.

## Upgrade notice (a newer major is out)

`GET /api/v1/version/upgrade?current_version=<this app's version>` returns
`{ major, message, url }` when a maintainer has switched on an announcement for a major
higher than this install's, and `204` otherwise. It's configured from the dashboard's
Releases page, so it can be switched on, reworded, or switched off without an app release.

- Fetched by `services::upgrade_notice` (command `fetch_upgrade_notice`, which sends
  `app.package_info().version`) whenever the Changelog page is opened, alongside the
  update check. Not at launch, and there's no sidebar badge for it.
- Shown as a banner on `ChangelogView.vue`. Its button opens `url` in the system browser
  through the existing `open_external_link` command. **It never downloads or installs
  anything**: moving to a new major is the user's decision, made on the website.
- Any failure (offline, `5xx`, unexpected body) just means no banner. It's never cached,
  since a cached "v4 is out" would outlive the maintainer switching it off. The client also
  drops a notice for its own major (or an older one) and any non-http(s) `url`, even though
  the backend shouldn't send either.

This code has to be in a line's **first** release to be useful there: a 3.x install that
shipped without it can't learn to show the banner until it updates to a 3.x release that
has it.

## What triggers a request

Fired once per app launch (`AppSidebar.vue`'s `onMounted`, so the sidebar badge can
light up without the user having visited Changelog) and again whenever the Changelog
page is opened (`ChangelogView.vue`'s `onActivated`). No polling loop while the app
stays open.

## What's sent — and not sent

The request is a plain, unauthenticated `GET`. `tauri-plugin-updater` only substitutes
`{{target}}`/`{{arch}}`/`{{current_version}}` into the request when the endpoint URL
literally contains those placeholders — it does **not** append them automatically to a
bare URL. `tauri.conf.json`'s endpoint is therefore
`.../version/latest?target={{target}}&arch={{arch}}&current_version={{current_version}}`,
not the bare path. No device id, telemetry, or other user data rides on it, same "small
non-sensitive public read" framing as `services/changelog.rs`'s existing changelog fetch.

`current_version` is optional on the backend side — omitting it (any other caller of this
same endpoint) just gets the latest published release back unconditionally as `200`. With
it, the backend itself short-circuits to `204` when the requester is already current,
which is a minor optimization: `tauri-plugin-updater` also compares `version` client-side
and silently no-ops on a same-or-lower version regardless (Rezure never passes
`allowDowngrades`), so a missing/blank `current_version` never produces a false update
prompt — it just costs one avoidable response body.

## Local testing without the production key

The backend endpoint is live, but a real update needs a release signed with the production
key (a release-time secret this repo never holds). The updater trusts exactly one public
key, baked into the binary at build time, so the app side is tested with an app that
trusts a key we hold instead: `scripts/test-updater.ps1`.

```powershell
.\scripts\test-updater.ps1                  # build once, run all three scenarios
.\scripts\test-updater.ps1 -Run wrong-key   # one scenario, reusing the build
.\scripts\test-updater.ps1 -Run serve       # serve "good" for a manual install test
.\scripts\test-updater.ps1 -RealUpdate -Rebuild   # also build a genuine higher version
```

What it does, with no change to the source or to `tauri.conf.json`:

1. Generates a throwaway dev key, and a second "wrong" one, under `%TEMP%\rezure-updater-test`.
2. Builds Rezure with the dev key as its trusted key and `http://127.0.0.1:8777/latest.json`
   as its endpoint, through a `--config` override file (`dangerousInsecureTransportProtocol`
   allows the plain-HTTP localhost endpoint; a normal build never has it). The bundled
   PHP/nginx payload is left out, since it has nothing to do with updating.
3. Signs the installer and writes four manifests: `good`, `launch` (a harmless `.exe`, signed
   correctly), `wrong-key` (signed by another key) and `tampered` (right signature, one bit
   flipped).
4. Runs the real binary against each (`scripts/updater-probe.mjs`), in its own `REZURE_HOME`
   and WebView2 profile with usage data off, and reads what the Changelog page shows.

Expected: `good` shows "Rezure 3.0.1 is available". `launch` presses Update on a signed
stand-in installer (a copy of Windows' own `hostname.exe`): the app downloads it, accepts
the signature, runs it and quits, which is what it does with a real installer, minus the
installing. `wrong-key` and `tampered` show the update too and then, on **Update**, download
the file and refuse with the updater's own signature error, leaving the version unchanged.
`good` does not press Update by default, because that installs a new Rezure on the machine
running the test; `-Install` does, and `-Run serve` serves it for doing it by hand.

The probe only counts a refusal if the error is about the signature **and** the installer
was actually downloaded first. That is deliberate: the first version of this test accepted
any red text, and passed on a script error ("Cannot read private member…") that meant the
Update button never worked at all.

Things to know:

- The installer is built with the same app identity as a real install, so **installing it
  replaces an installed Rezure**. Test on a machine (or VM) where that is fine.
- Unless `-RealUpdate` is given, the offered installer is the one just built, so the
  manifest says a higher version than the installer is. Detection, download and signature
  checks are real; "the version changed afterwards" is only proven with `-RealUpdate`.
- A failed update *check* (a broken manifest, say) is not shown in the UI: `checkError` is
  kept in the store but nothing renders it, so it looks like "no update". The probe's
  `none` expectation cannot tell the two apart.
- The signing half can be checked without the private key: a release's `.sig` verifies
  against the `pubkey` in `tauri.conf.json` with `minisign-verify`, which is what the
  plugin uses. Done for the 3.0.0 NSIS and MSI installers (verified; a different file and
  a single flipped bit are both rejected).
