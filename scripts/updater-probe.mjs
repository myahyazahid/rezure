#!/usr/bin/env node
// Drives the REAL Rezure binary through its auto-update flow and reports what
// the user would see. Part of scripts/test-updater.ps1; usable on its own.
//
//   node scripts/updater-probe.mjs --exe <rezureapp.exe> --expect none
//   node scripts/updater-probe.mjs --exe <rezureapp.exe> --dir <serve> --scenario wrong-key
//   node scripts/updater-probe.mjs --dir <serve> --serve-only          # for a manual install test
//
// What it does, and why it is safe to run on a working machine:
//   * the app runs with its own REZURE_HOME and WebView2 profile in the temp
//     folder, with usage data off — it touches neither C:\rezure nor the
//     telemetry of a real install;
//   * the page is read and clicked through WebView2's DevTools port
//     (--remote-debugging-port), so no UI-automation dependency is needed;
//   * "good" stops at "an update is available" unless --install is given,
//     because installing replaces the app on this machine. "wrong-key" and
//     "tampered" do press Update: the updater checks the signature before it
//     runs anything, so a correct build refuses and nothing is installed.
//
// The updater endpoint is baked into the binary at build time (see
// test-updater.ps1), so nothing here can point a normal build at a fake server.

import { spawn, execFileSync } from 'node:child_process'
import { createServer } from 'node:http'
import { copyFileSync, createReadStream, existsSync, mkdirSync, mkdtempSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

function parseArgs(argv) {
  const out = {}
  for (let i = 0; i < argv.length; i++) {
    if (!argv[i].startsWith('--')) continue
    const key = argv[i].slice(2)
    const next = argv[i + 1]
    if (next === undefined || next.startsWith('--')) out[key] = true
    else {
      out[key] = next
      i++
    }
  }
  return out
}

const args = parseArgs(process.argv.slice(2))
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

// ---- the fake release server --------------------------------------------------

/** Every file the app asked for, so a test can tell "refused after downloading". */
const requested = []

function serve(dir, port, scenario) {
  if (scenario) {
    const manifest = join(dir, `latest.${scenario}.json`)
    if (!existsSync(manifest)) throw new Error(`no manifest for scenario "${scenario}": ${manifest}`)
    copyFileSync(manifest, join(dir, 'latest.json'))
  }
  const server = createServer((req, res) => {
    const name = decodeURIComponent((req.url ?? '/').split('?')[0]).replace(/^\/+/, '')
    const file = join(dir, name)
    // Plain files from one folder; nothing outside it.
    if (!name || name.includes('..') || !existsSync(file) || !statSync(file).isFile()) {
      res.writeHead(404).end()
      return
    }
    res.writeHead(200, { 'Content-Length': statSync(file).size })
    createReadStream(file).pipe(res)
    requested.push(name)
    console.log(`  [server] ${req.method} /${name}`)
  })
  return new Promise((resolve) => server.listen(port, '127.0.0.1', () => resolve(server)))
}

// ---- DevTools Protocol, just enough ----------------------------------------------

async function connect(port, timeoutMs = 60000) {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    try {
      const pages = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()
      const page = pages.find((p) => p.type === 'page' && p.webSocketDebuggerUrl)
      if (page) {
        const ws = new WebSocket(page.webSocketDebuggerUrl)
        await new Promise((resolve, reject) => {
          ws.onopen = resolve
          ws.onerror = reject
        })
        return ws
      }
    } catch {
      // not listening yet
    }
    await sleep(500)
  }
  throw new Error('the app never opened its DevTools port')
}

function session(ws) {
  let id = 0
  const pending = new Map()
  ws.onmessage = (event) => {
    const message = JSON.parse(event.data)
    pending.get(message.id)?.(message)
  }
  return async function evaluate(expression) {
    const myId = ++id
    const reply = new Promise((resolve) => pending.set(myId, resolve))
    ws.send(
      JSON.stringify({
        id: myId,
        method: 'Runtime.evaluate',
        params: { expression, returnByValue: true, awaitPromise: true },
      }),
    )
    const message = await reply
    if (message.result?.exceptionDetails) {
      throw new Error(message.result.exceptionDetails.exception?.description ?? 'evaluate failed')
    }
    return message.result?.result?.value
  }
}

async function waitFor(evaluate, expression, timeoutMs, what) {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    const value = await evaluate(expression)
    if (value) return value
    await sleep(400)
  }
  throw new Error(`timed out waiting for ${what}`)
}

// ---- the run -------------------------------------------------------------------

const port = Number(args.port ?? 8777)

if (args['serve-only']) {
  if (!args.dir) throw new Error('--serve-only needs --dir')
  await serve(args.dir, port, args.scenario)
  console.log(`serving ${args.dir} on http://127.0.0.1:${port}/latest.json — Ctrl+C to stop`)
  await new Promise(() => {})
}

if (!args.exe) throw new Error('--exe <path to rezureapp.exe> is required')

const expectation =
  args.expect ??
  { good: 'available', launch: 'launches', 'wrong-key': 'rejected', tampered: 'rejected' }[
    args.scenario
  ] ??
  'none'
const server = args.dir ? await serve(args.dir, port, args.scenario) : null

const home = mkdtempSync(join(tmpdir(), 'rezure-probe-home-'))
const profile = mkdtempSync(join(tmpdir(), 'rezure-probe-webview-'))
mkdirSync(join(home, 'etc'), { recursive: true })
writeFileSync(join(home, 'etc', 'settings.json'), JSON.stringify({ shareUsageData: false }))
const devtoolsPort = 9300 + Math.floor(Math.random() * 600)

const app = spawn(args.exe, [], {
  env: {
    ...process.env,
    REZURE_HOME: home,
    WEBVIEW2_USER_DATA_FOLDER: profile,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${devtoolsPort}`,
  },
  stdio: 'ignore',
})
// On Windows the updater hands the downloaded installer to the shell and then
// quits the app itself; that exit is how "it accepted the file and ran it" shows.
const appExited = new Promise((resolve) => app.once('exit', (code) => resolve(code)))

const report = { expect: expectation, scenario: args.scenario ?? null }
let ws = null
try {
  ws = await connect(devtoolsPort)
  const evaluate = session(ws)

  await waitFor(evaluate, `document.body && document.body.innerText.includes('Changelog')`, 60000, 'the app to load')
  // The version label is filled in after the page, so wait for it.
  const versionLabel = `(document.body.innerText.match(/v(\\d+\\.\\d+\\.\\d+)/) || [])[1] || null`
  report.appVersion = await waitFor(evaluate, versionLabel, 30000, 'the version label')

  // The update check runs at launch and again whenever Changelog is opened.
  await evaluate(`[...document.querySelectorAll('a')].find((a) => /Changelog/.test(a.textContent))?.click()`)
  await waitFor(evaluate, `/What's new in Rezure/.test(document.body.innerText)`, 20000, 'the Changelog page')
  await waitFor(evaluate, `!/Checking for updates/.test(document.body.innerText)`, 60000, 'the update check to finish')

  report.available = await evaluate(`(document.body.innerText.match(/Rezure (\\d+\\.\\d+\\.\\d+) is available/) || [])[1] || null`)

  if (expectation === 'none') {
    report.ok = report.available === null
  } else if (expectation === 'available') {
    report.ok = report.available !== null && (!args.version || report.available === args.version)
    if (report.ok && args.install) {
      console.log('  pressing Update — this installs the new version on THIS machine')
      await evaluate(`[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'Update')?.click()`)
      await sleep(30000)
    }
  } else if (expectation === 'launches') {
    // The offered file is a harmless .exe signed with the right key. Accepting
    // it means: downloaded, signature checked, run, and the app quits.
    if (report.available === null) {
      report.ok = false
      report.problem = 'no update was offered'
    } else {
      await evaluate(`[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'Update')?.click()`)
      const outcome = await Promise.race([appExited.then((code) => ({ exited: true, code })), sleep(90000).then(() => ({ exited: false }))])
      report.downloaded = requested.some((name) => name === 'launch-check.exe')
      report.appExited = outcome.exited
      report.ok = report.downloaded && outcome.exited
    }
  } else if (expectation === 'rejected') {
    if (report.available === null) {
      report.ok = false
      report.problem = 'no update was offered, so there was nothing to reject'
    } else {
      await evaluate(`[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'Update')?.click()`)
      // Refusal shows as the red line under the button; the button returns to "Update".
      report.error = await waitFor(
        evaluate,
        `document.querySelector('p.text-red-700')?.innerText || null`,
        90000,
        'the updater to refuse the download',
      )
      report.appVersionAfter = await evaluate(versionLabel)
      // The refusal must be the updater's own, about the signature — not some
      // other failure that happens to put red text on screen (a script error
      // once did exactly that) — and it must come AFTER the installer was
      // fetched, since the signature is checked on the downloaded bytes.
      report.downloaded = requested.some((name) => name.endsWith('.exe'))
      report.aboutSignature = /signature|minisign|verif|key/i.test(report.error)
      report.ok =
        report.downloaded && report.aboutSignature && report.appVersionAfter === report.appVersion
    }
  }
} catch (error) {
  report.ok = false
  report.problem = String(error.message ?? error)
} finally {
  try {
    ws?.close()
  } catch {
    // already closed
  }
  try {
    execFileSync('taskkill', ['/PID', String(app.pid), '/T', '/F'], { stdio: 'ignore' })
  } catch {
    // already gone
  }
  server?.close()
  await sleep(1000)
  for (const dir of [home, profile]) rmSync(dir, { recursive: true, force: true })
}

console.log(JSON.stringify(report, null, 2))
process.exit(report.ok ? 0 : 1)
