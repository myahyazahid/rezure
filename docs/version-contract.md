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

## Local testing without a real signed installer

The backend endpoint is live, but exercising a real update still needs a release signed
with the production key (a release-time secret this repo never holds). Until you have one,
the app-side flow can be exercised end-to-end against a fake manifest instead:

1. Generate a throwaway *dev* signing keypair (`tauri signer generate`) — separate
   from the real release key.
2. Build and sign a Rezure installer at a higher version number with that dev key.
3. Serve a hand-written `latest.json` matching the shape above (any static file
   server works) pointing `url` at that installer and `signature` at its `.sig`.
4. Point a debug build's `plugins.updater.endpoints` at that local URL (e.g. gated
   behind `cfg!(debug_assertions)` in `src-tauri/src/lib.rs`) with the dev pubkey
   swapped into `tauri.conf.json` for that build.
5. Confirm the update is detected, downloads with progress, and installs — and
   separately confirm a manifest signed with a *different* key is rejected, proving
   the signature check is actually enforced rather than silently skipped.
