//! Generates Nginx virtual host configs, one per detected project, plus
//! Rezure's own top-level nginx config that `include`s them.
//!
//! The extracted binary's own `conf/nginx.conf` (under
//! `binaries::install_root()`) is never touched — it stays pristine,
//! matching what was downloaded and checksum-verified. Rezure generates
//! its own main config under its runtime data dir instead, and points
//! nginx at it with `-c` / `-p`.
//!
//! There's no PHP-FPM on Windows, so every vhost proxies PHP requests to
//! `php-cgi -b 127.0.0.1:PORT` responders (see `services::process`) — the
//! standard substitute FastCGI responder on this platform. Each PHP version
//! runs several of them, so a vhost names an nginx `upstream` holding that
//! version's workers rather than one port; the upstreams themselves live in
//! one generated file next to the vhosts (see [`upstreams_config`]).
//!
//! A project's framework picks its serve profile (`services::frameworks`):
//! `php` gets the FastCGI block above, `proxy` a block that passes every
//! request to the project's dev server on its own port
//! ([`proxy_vhost_config`], `services::dev_servers`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::binaries;
use super::dev_servers;
use super::frameworks::ServeProfile;
use super::php;
use super::php_ini;
use super::php_pool;
use super::projects::scan_projects;
use crate::db::projects::ProjectInfo;
use crate::utils::command::HiddenWindow;
use crate::utils::error::AppError;
use crate::utils::paths;

/// `%LOCALAPPDATA%\Rezure\data\nginx` — Rezure's own generated nginx
/// config, logs, and temp directories (separate from the pristine
/// extracted binary under `binaries::install_root()`).
pub fn nginx_runtime_dir() -> Result<PathBuf, AppError> {
    Ok(paths::data()?.join("nginx"))
}

pub fn vhosts_dir() -> Result<PathBuf, AppError> {
    Ok(nginx_runtime_dir()?.join("vhosts"))
}

/// Every PHP `upstream` block a vhost can name. Outside [`vhosts_dir`] on
/// purpose: that folder is pruned down to one `.conf` per project.
fn upstreams_path() -> Result<PathBuf, AppError> {
    Ok(nginx_runtime_dir()?.join("php-upstreams.conf"))
}

fn nginx_conf_dir() -> Result<PathBuf, AppError> {
    let exe = binaries::exe_path(binaries::find("nginx")?)?;
    exe.parent()
        .map(|dir| dir.join("conf"))
        .ok_or_else(|| AppError::Io("nginx.exe has no parent directory".to_string()))
}

/// Windows nginx parses `\t`/`\n` *inside config string values* as escape
/// sequences (a real backslash-path bug, not a hypothetical one — a path
/// like `...\nginx.pid` silently becomes "...", newline, "ginx.pid").
/// Forward slashes sidestep it entirely and Windows accepts them fine.
fn conf_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

fn ensure_dir(path: &Path) -> Result<(), AppError> {
    fs::create_dir_all(path)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", path.display())))
}

/// Writes (overwriting) Rezure's top-level nginx config — `http` defaults,
/// explicit temp-dir paths (nginx's own relative defaults don't
/// auto-create their parent directories), and an `include` of every
/// generated vhost. Returns the config's path, ready for `-c`.
pub fn ensure_main_config(nginx_exe: &Path) -> Result<PathBuf, AppError> {
    let runtime = nginx_runtime_dir()?;
    let vhosts = vhosts_dir()?;
    ensure_dir(&runtime.join("logs"))?;
    ensure_dir(&runtime.join("temp").join("client_body"))?;
    ensure_dir(&runtime.join("temp").join("proxy"))?;
    ensure_dir(&runtime.join("temp").join("fastcgi"))?;
    ensure_dir(&vhosts)?;

    // `sync_vhosts` writes the real one, but nginx refuses to start on an
    // `include` of a file that doesn't exist — which it wouldn't yet if
    // nginx is started before any sync ever ran.
    let upstreams = upstreams_path()?;
    if !upstreams.is_file() {
        fs::write(&upstreams, upstreams_config(&[php_pool::DEFAULT_BASE_PORT]))
            .map_err(|e| AppError::Io(format!("could not write {}: {e}", upstreams.display())))?;
    }

    let mime_types = nginx_exe
        .parent()
        .map(|dir| dir.join("conf").join("mime.types"))
        .ok_or_else(|| AppError::Io("nginx.exe has no parent directory".to_string()))?;

    let config = format!(
        r#"worker_processes 1;
error_log "{error_log}";
pid "{pid_file}";

events {{
    worker_connections 1024;
}}

http {{
    include "{mime_types}";
    default_type application/octet-stream;
    client_body_temp_path "{client_body_temp}";
    proxy_temp_path "{proxy_temp}";
    fastcgi_temp_path "{fastcgi_temp}";
    sendfile on;
    # nginx's default hash bucket is too small for most real project names
    # (e.g. "my-awesome-app.test" already overflows it) — nginx refuses to
    # start at all rather than truncate, so this has to be raised up front.
    server_names_hash_bucket_size 64;
    # Kept in step with the generated php.ini (`services::php_ini`): nginx's
    # own defaults (1 MB body, 60s FastCGI timeouts) sit far below PHP's, so
    # uploads 413 and slow requests time out before PHP gets a say.
    client_max_body_size {body_size_limit_mb}m;
    fastcgi_read_timeout {fastcgi_timeout}s;
    fastcgi_send_timeout {fastcgi_timeout}s;
    # A worker that's down (crashed, restarting) refuses the connection, and
    # `error` sends the request on to the next one. `timeout` is left out
    # deliberately: a request that ran out the clock would otherwise be
    # re-run on every other worker in turn, tying all of them up.
    fastcgi_next_upstream error;

    # Dev servers' hot reload is a websocket: the upgrade has to be passed on,
    # and an ordinary request must not ask the upstream to keep it open.
    map $http_upgrade $connection_upgrade {{
        default upgrade;
        '' close;
    }}

    # Catches any request whose Host header doesn't match a vhost below.
    server {{
        listen 80 default_server;
        server_name _;
        return 404;
    }}

    include "{upstreams}";
    include "{vhosts_glob}";
}}
"#,
        error_log = conf_path(&runtime.join("logs").join("error.log")),
        pid_file = conf_path(&runtime.join("nginx.pid")),
        mime_types = conf_path(&mime_types),
        client_body_temp = conf_path(&runtime.join("temp").join("client_body")),
        proxy_temp = conf_path(&runtime.join("temp").join("proxy")),
        fastcgi_temp = conf_path(&runtime.join("temp").join("fastcgi")),
        upstreams = conf_path(&upstreams),
        vhosts_glob = conf_path(&vhosts.join("*.conf")),
        body_size_limit_mb = php_ini::BODY_SIZE_LIMIT_MB,
        fastcgi_timeout = php_ini::MAX_EXECUTION_TIME_SECS,
    );

    let config_path = runtime.join("nginx.conf");
    fs::write(&config_path, config)
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", config_path.display())))?;
    ensure_pages()?;
    Ok(config_path)
}

/// `<runtime>\pages` — the pages nginx serves itself, rather than a project.
fn pages_dir() -> Result<PathBuf, AppError> {
    Ok(nginx_runtime_dir()?.join("pages"))
}

/// What a `proxy` project shows while its dev server isn't running.
const DEV_SERVER_DOWN_PAGE: &str = "rezure-dev-server-down.html";

/// Its contents. A dev server that's off is the normal state of a `proxy`
/// project, not a fault, so nginx's bare "502 Bad Gateway" would point the
/// user at the wrong thing. Reloads itself, so starting the dev server from
/// Rezure brings the page up without another click.
const DEV_SERVER_DOWN_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="refresh" content="4">
<title>Dev server not running</title>
<style>
  :root { color-scheme: light dark; }
  body { margin: 0; min-height: 100vh; display: grid; place-items: center;
         font: 16px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif;
         background: #f6f5fb; color: #1f1d2b; }
  main { max-width: 32rem; padding: 2rem; }
  h1 { font-size: 1.25rem; margin: 0 0 .5rem; }
  p { margin: .5rem 0; color: #5b5870; }
  code { font: 0.9em ui-monospace, Consolas, monospace; }
  @media (prefers-color-scheme: dark) {
    body { background: #15131d; color: #ece9f6; }
    p { color: #a9a5bd; }
  }
</style>
</head>
<body>
<main>
  <h1>This project's dev server isn't running</h1>
  <p>Start it from Rezure: click <strong>Open</strong> on the project, or
  <strong>Projects → Dev servers</strong>. This page reloads on its own.</p>
  <p>Started it yourself? Check it listens on the port shown in Dev servers.</p>
</main>
</body>
</html>
"#;

/// Writes the pages under [`pages_dir`] when they aren't current.
fn ensure_pages() -> Result<(), AppError> {
    let dir = pages_dir()?;
    ensure_dir(&dir)?;
    write_if_changed(&dir.join(DEV_SERVER_DOWN_PAGE), DEV_SERVER_DOWN_HTML)?;
    Ok(())
}

/// One `upstream` per PHP port block in `bases`, each listing that
/// version's workers.
///
/// `least_conn` rather than nginx's default round-robin: a `php-cgi` busy
/// with a slow request still *accepts* new connections (the OS queues them
/// in its listen backlog), so round-robin would keep handing requests to it
/// and they'd wait behind the slow one — the exact stall the workers exist
/// to avoid. `least_conn` sends each request to the worker with the fewest
/// requests in flight, i.e. an idle one whenever there is one.
fn upstreams_config(bases: &[u16]) -> String {
    let mut config = String::from(
        "# Generated by Rezure from services::php_pool - rewritten on every project sync.\n",
    );
    for &base in bases {
        config.push_str(&format!(
            "\nupstream {} {{\n    least_conn;\n",
            php_pool::upstream_name(base)
        ));
        for port in php_pool::worker_ports(base) {
            config.push_str(&format!("    server 127.0.0.1:{port};\n"));
        }
        config.push_str("}\n");
    }
    config
}

/// Writes `contents` to `path` unless it already holds exactly that.
/// Returns whether it wrote — the same "don't make nginx reload for
/// nothing" rule [`VhostSync::changed`] exists for.
fn write_if_changed(path: &Path, contents: &str) -> Result<bool, AppError> {
    if fs::read_to_string(path).ok().as_deref() == Some(contents) {
        return Ok(false);
    }
    fs::write(path, contents)
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", path.display())))?;
    Ok(true)
}

/// One `php` project's `server` block. `docroot` is already resolved — the
/// framework (and any override) decided it in `services::frameworks`.
///
/// Every dotfile and dotfolder answers 404, `.well-known` aside. A project
/// served from its own folder (CodeIgniter 3, WordPress, plain PHP) keeps
/// its `.env` and `.git` right there, and `share` can put the site on the
/// internet — matching only `.ht*`, as this once did, handed both out.
fn vhost_config(domain: &str, docroot: &Path, fastcgi_params: &Path, upstream: &str) -> String {
    format!(
        r#"server {{
    listen 80;
    server_name {domain};
    root "{root}";
    index index.php index.html;

    location ~ /\.(?!well-known(?:/|$)) {{
        return 404;
    }}

    location / {{
        try_files $uri $uri/ /index.php?$query_string;
    }}

    location ~ \.php$ {{
        fastcgi_pass {upstream};
        fastcgi_index index.php;
        fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;
        include "{fastcgi_params}";
    }}
}}
"#,
        domain = domain,
        root = conf_path(docroot),
        upstream = upstream,
        fastcgi_params = conf_path(fastcgi_params),
    )
}

/// The nginx `upstream` name for project `id`'s dev server. Spelled out
/// byte by byte for anything but a letter or digit, so two ids can never map
/// to one name and nothing in an id reaches the config as syntax.
fn dev_upstream_name(id: &str) -> String {
    let mut name = String::from("rezure_dev_");
    for byte in id.bytes() {
        if byte.is_ascii_alphanumeric() {
            name.push(byte as char);
        } else {
            name.push_str(&format!("_{byte:02x}"));
        }
    }
    name
}

/// One `proxy` project's `server` block: every request to `domain` passed
/// to its dev server on `port`.
///
/// - Both loopback addresses, IPv6 as the backup: Vite listens on
///   `localhost`, which on most Windows machines is `::1` only, while Next
///   and most Node apps answer on `127.0.0.1`.
/// - `Upgrade`/`Connection` carry the hot-reload websocket through.
/// - `Host` stays the project's domain, so a framework that builds absolute
///   URLs from it (Next, Nuxt) links to `astro.test`, not `localhost:4400`.
/// - Buffering off and a long read timeout: streamed responses (server-sent
///   events, 9router's LLM streams) arrive as they're written, and an idle
///   hot-reload connection isn't cut.
/// - Next's own dev endpoints (`/_next/*`, `/__nextjs*`) get their `Origin`
///   dropped when it's the project's own domain. Next 16 answers 403 to any
///   of them with an `Origin` not on `localhost` or its `allowedDevOrigins`,
///   and the hot-reload websocket always sends one — the page then never
///   finishes loading. An `Origin` from any other site still goes through,
///   so Next keeps refusing those.
/// - No dotfile rule here, unlike [`vhost_config`]: Vite serves its own
///   modules from `/node_modules/.vite/`, and the dev server decides what it
///   exposes.
/// - nginx's own 502 (nothing listening) becomes a page that says the dev
///   server is off. One the app itself returns passes through untouched.
fn proxy_vhost_config(domain: &str, id: &str, port: u16, pages: &Path) -> String {
    let upstream = dev_upstream_name(id);
    // `domain` passed `projects::is_safe_domain`: letters, digits, hyphens
    // and dots, so escaping the dots is all a regex needs.
    let own_origin = domain.replace('.', r"\.");
    let proxy = |extra: &str| {
        format!(
            r#"        proxy_pass http://{upstream};
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection $connection_upgrade;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header X-Forwarded-Host $host;
{extra}        proxy_buffering off;
        proxy_read_timeout 1h;
        proxy_send_timeout 1h;
"#
        )
    };
    format!(
        r#"upstream {upstream} {{
    server 127.0.0.1:{port};
    server [::1]:{port} backup;
}}

server {{
    listen 80;
    server_name {domain};

    set $rezure_dev_origin $http_origin;
    if ($http_origin ~* "^https?://{own_origin}(:[0-9]+)?$") {{
        set $rezure_dev_origin "";
    }}

    location / {{
{root}    }}

    location ~ ^/(_next/|__nextjs) {{
{next}    }}

    error_page 502 504 /{page};
    location = /{page} {{
        root "{pages}";
        internal;
    }}
}}
"#,
        root = proxy(""),
        next = proxy("        proxy_set_header Origin $rezure_dev_origin;\n"),
        page = DEV_SERVER_DOWN_PAGE,
        pages = conf_path(pages),
    )
}

/// What a [`sync_vhosts`] pass did.
#[derive(Debug, Clone)]
pub struct VhostSync {
    /// How many vhosts are active.
    pub active: usize,
    /// The projects that got one, as served — `dev_port` filled in for each
    /// `proxy` project, including a port picked by this pass. What the dev
    /// servers are reconciled against (`dev_servers::specs`), so they're
    /// built from the same scan the vhosts were.
    pub projects: Vec<ProjectInfo>,
    /// Whether any `.conf` was actually written or deleted.
    ///
    /// The point of tracking this is that `sync_vhosts` runs on every visit
    /// to the Projects page. Reloading nginx unconditionally would cycle its
    /// workers each time the user opened a tab; reloading only on a real
    /// change makes the reload free when nothing moved.
    pub changed: bool,
}

/// Rewrites every project's vhost to match the current scan of
/// `projects::www_root()`, deleting `.conf` files for projects that no
/// longer exist. Only touches the files on disk — call [`reload`] afterward,
/// when [`VhostSync::changed`] is set, to make a running nginx pick it up.
///
/// `php_versions` is each project's SQLite-stored PHP version override
/// (`db::projects::fetch_php_versions`) — a filesystem scan alone has no way
/// to know it (a folder has no opinion on PHP version), same reason
/// `commands::projects::list_projects` merges it in separately for the
/// frontend. Without this, every project would resolve to the global
/// default port regardless of what it's pinned to, silently making every
/// pin a no-op.
///
/// `resolve_pool` is handed the set of versions this pass' projects need a
/// pooled instance for (via `php_pool::wanted_versions`) and must return
/// their actual port blocks — in practice always `ServiceManager::sync_php_pool`,
/// the one thing that knows which port an already-registered pooled
/// service is really bound to. Taking a closure rather than computing ports
/// here directly is deliberate: an earlier version of this function
/// computed them independently, and that's exactly what let a pinned
/// project's port silently move out from under its own already-running
/// `php-cgi` (see `php_pool::assign_ports`'s doc comment) — routing every
/// caller through `ServiceManager` is what makes that impossible now.
pub fn sync_vhosts(
    php_versions: &HashMap<String, Option<String>>,
    resolve_pool: impl FnOnce(&BTreeSet<String>) -> BTreeMap<String, u16>,
) -> Result<VhostSync, AppError> {
    let vhosts = vhosts_dir()?;
    ensure_dir(&vhosts)?;
    ensure_pages()?;
    let fastcgi_params = nginx_conf_dir()?.join("fastcgi_params");
    let pages = pages_dir()?;

    // A linked project whose folder is gone gets no vhost — nginx would be
    // pointed at a root that doesn't exist. It stays in the list, though,
    // since the folder may simply be on a drive that isn't plugged in.
    let mut current: Vec<_> = scan_projects()?
        .into_iter()
        .filter(|project| !project.missing && !project.domain_invalid)
        .collect();
    // A `proxy` project needs its port to have a vhost at all, so one that
    // has none yet is given one here, before anything is written.
    let dev_ports = dev_servers::assign_ports(&current);
    for project in &mut current {
        if let Some(version) = php_versions.get(&project.id) {
            project.php_version = version.clone();
        }
        if project.serve == ServeProfile::Proxy {
            project.dev_port = dev_ports.get(&project.id).copied();
        }
    }
    let current_ids: HashSet<&str> = current.iter().map(|p| p.id.as_str()).collect();

    let installed: BTreeSet<String> = php::installed().into_iter().map(|r| r.version).collect();
    let global_active = php::active_id();
    let wanted = php_pool::wanted_versions(&current, &installed, &global_active);
    let pool = resolve_pool(&wanted);

    // Written before any vhost, so a vhost never names an upstream nginx
    // hasn't been told about. Stale versions' upstreams are harmless to keep
    // until the next sync drops them — nothing names them any more.
    let mut bases: Vec<u16> = vec![php_pool::DEFAULT_BASE_PORT];
    bases.extend(pool.values().copied());
    let mut changed = write_if_changed(&upstreams_path()?, &upstreams_config(&bases))?;

    // Drop vhosts for projects that no longer exist on disk.
    if let Ok(entries) = fs::read_dir(&vhosts) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_stale = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|stem| !current_ids.contains(stem))
                .unwrap_or(false);
            if is_stale
                && path.extension().is_some_and(|ext| ext == "conf")
                && fs::remove_file(&path).is_ok()
            {
                changed = true;
            }
        }
    }

    let mut active = 0;
    for project in &current {
        let path = vhosts.join(format!("{}.conf", project.id));
        let config = match (project.serve, project.dev_port) {
            (ServeProfile::Php, _) => {
                let base = php_pool::port_for_project(
                    project.php_version.as_deref(),
                    &pool,
                    php_pool::DEFAULT_BASE_PORT,
                );
                vhost_config(
                    &project.domain,
                    Path::new(&project.docroot),
                    &fastcgi_params,
                    &php_pool::upstream_name(base),
                )
            }
            (ServeProfile::Proxy, Some(port)) => {
                proxy_vhost_config(&project.domain, &project.id, port, &pages)
            }
            // Every port in the range is taken (`assign_ports` logged it).
            // No vhost beats one pointing at somebody else's dev server.
            (ServeProfile::Proxy, None) => {
                if fs::remove_file(&path).is_ok() {
                    changed = true;
                }
                continue;
            }
        };

        // Compared before writing, so an unchanged project neither churns the
        // file's mtime nor reports a change nginx would have to reload for.
        changed |= write_if_changed(&path, &config)?;
        active += 1;
    }

    Ok(VhostSync {
        active,
        changed,
        projects: current,
    })
}

/// Tells a running nginx to reload its config — picks up a project just
/// added or removed immediately instead of waiting for nginx's next full
/// restart. Callers are expected to check the service is actually running
/// first (see `commands::projects`'s callers): asking a stopped nginx to
/// reload just fails, harmlessly but noisily.
pub fn reload() -> Result<(), AppError> {
    let nginx_exe = binaries::exe_path(binaries::find("nginx")?)?;
    let config_path = ensure_main_config(&nginx_exe)?;
    let runtime = nginx_runtime_dir()?;

    let status = Command::new(&nginx_exe)
        .arg("-s")
        .arg("reload")
        .arg("-c")
        .arg(&config_path)
        .arg("-p")
        .arg(&runtime)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .hidden()
        .status()
        .map_err(|e| AppError::Io(format!("could not run nginx -s reload: {e}")))?;

    if !status.success() {
        return Err(AppError::Io(format!(
            "nginx -s reload exited with {status}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conf_path_uses_forward_slashes_even_for_windows_input() {
        let path = Path::new(r"C:\Users\dev\rezure\nginx.conf");
        assert_eq!(conf_path(path), "C:/Users/dev/rezure/nginx.conf");
    }

    #[test]
    fn vhost_config_embeds_domain_root_and_fastcgi_upstream() {
        let config = vhost_config(
            "blog.test",
            Path::new(r"C:\Users\dev\rezure\www\blog"),
            Path::new(r"C:\nginx\conf\fastcgi_params"),
            &php_pool::upstream_name(php_pool::DEFAULT_BASE_PORT),
        );

        assert!(config.contains("server_name blog.test;"));
        assert!(config.contains("root \"C:/Users/dev/rezure/www/blog\";"));
        assert!(config.contains("fastcgi_pass rezure_php_9100;"));
        assert!(config.contains("include \"C:/nginx/conf/fastcgi_params\";"));
    }

    /// `.env` and `.git` in a project served from its own folder must not be
    /// downloadable; `.well-known` (ACME, app links) must still be served.
    #[test]
    fn php_vhosts_hide_dotfiles_but_not_well_known() {
        let config = vhost_config(
            "blog.test",
            Path::new(r"C:\rezure\www\blog"),
            Path::new("fastcgi_params"),
            &php_pool::upstream_name(php_pool::DEFAULT_BASE_PORT),
        );
        assert!(
            config.contains(r"location ~ /\.(?!well-known(?:/|$)) {"),
            "{config}"
        );
        // Ahead of the PHP location: nginx takes the first regex that matches,
        // so `/.git/x.php` must hit the 404 before it can reach PHP.
        assert!(config.find("well-known").unwrap() < config.find(r"\.php$").unwrap());
    }

    #[test]
    fn proxy_vhosts_pass_everything_to_the_dev_server() {
        let config = proxy_vhost_config(
            "astro-web.test",
            "astro-web",
            4400,
            Path::new(r"C:\rezure\data\nginx\pages"),
        );

        assert!(
            config.contains("upstream rezure_dev_astro_2dweb {"),
            "{config}"
        );
        assert!(config.contains("server 127.0.0.1:4400;"));
        assert!(config.contains("server [::1]:4400 backup;"));
        assert!(config.contains("server_name astro-web.test;"));
        assert_eq!(
            config
                .matches("proxy_pass http://rezure_dev_astro_2dweb;")
                .count(),
            2
        );
        // Next's dev endpoints lose an Origin that's the project's own, and
        // only those.
        assert!(config.contains(r#"if ($http_origin ~* "^https?://astro-web\.test(:[0-9]+)?$") {"#));
        assert!(config.contains("location ~ ^/(_next/|__nextjs) {"));
        assert_eq!(
            config
                .matches("proxy_set_header Origin $rezure_dev_origin;")
                .count(),
            1
        );
        assert!(config.contains("proxy_set_header Upgrade $http_upgrade;"));
        assert!(config.contains("proxy_set_header Connection $connection_upgrade;"));
        assert!(config.contains("proxy_set_header Host $host;"));
        assert!(config.contains("proxy_buffering off;"));
        assert!(config.contains(&format!("error_page 502 504 /{DEV_SERVER_DOWN_PAGE};")));
        assert!(config.contains("root \"C:/rezure/data/nginx/pages\";"));
        // Nothing of the PHP profile, and nothing served from the folder.
        assert!(!config.contains("fastcgi_pass"));
        assert!(!config.contains("try_files"));
        assert_eq!(config.matches('{').count(), config.matches('}').count());
    }

    /// Two ids that differ only in punctuation still get two upstreams —
    /// a shared name would make nginx refuse the whole config.
    #[test]
    fn dev_upstream_names_are_distinct_and_plain() {
        let names = ["a-b", "a_b", "a.b", "ab"].map(dev_upstream_name);
        let unique: HashSet<&String> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "{names:?}");
        for name in &names {
            assert!(
                name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "{name}"
            );
        }
    }

    /// Both templates, and the main config's `map`, through the real nginx's
    /// own parser (`nginx -t` binds nothing). Run with
    /// `cargo test --lib services::vhosts::tests::the_real_nginx_accepts_both_profiles -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn the_real_nginx_accepts_both_profiles() {
        let nginx = binaries::exe_path(binaries::find("nginx").unwrap()).unwrap();
        let mime_types = nginx.parent().unwrap().join("conf").join("mime.types");
        let fastcgi_params = nginx.parent().unwrap().join("conf").join("fastcgi_params");
        // Not the temp folder: nginx fails to open its own `-c` under a
        // profile whose 8.3 name it can't take apart (`-LENOV~1`).
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("rezure-test-nginx-t-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("logs")).unwrap();
        fs::create_dir_all(dir.join("temp")).unwrap();

        let config = format!(
            r#"worker_processes 1;
error_log "{logs}/error.log";
pid "{logs}/nginx.pid";
events {{ worker_connections 16; }}
http {{
    include "{mime}";
    map $http_upgrade $connection_upgrade {{
        default upgrade;
        '' close;
    }}
{upstreams}
{php}
{proxy}
}}
"#,
            logs = conf_path(&dir.join("logs")),
            mime = conf_path(&mime_types),
            upstreams = upstreams_config(&[php_pool::DEFAULT_BASE_PORT]),
            php = vhost_config(
                "blog.test",
                &dir,
                &fastcgi_params,
                &php_pool::upstream_name(php_pool::DEFAULT_BASE_PORT),
            ),
            proxy = proxy_vhost_config("astro-web.test", "astro-web", 4400, &dir),
        );
        let path = dir.join("nginx.conf");
        fs::write(&path, config).unwrap();

        let output = Command::new(&nginx)
            .args(["-t", "-c"])
            .arg(&path)
            .arg("-p")
            .arg(&dir)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&output.stderr).to_string();
        println!("{said}");
        let _ = fs::remove_dir_all(&dir);
        assert!(output.status.success(), "{said}");
    }

    #[test]
    fn upstreams_list_every_worker_of_every_block_least_conn() {
        let config = upstreams_config(&[php_pool::DEFAULT_BASE_PORT, php_pool::BASE_PORT]);

        for base in [php_pool::DEFAULT_BASE_PORT, php_pool::BASE_PORT] {
            assert!(config.contains(&format!("upstream {} {{", php_pool::upstream_name(base))));
            for port in php_pool::worker_ports(base) {
                assert!(config.contains(&format!("server 127.0.0.1:{port};")));
            }
        }
        assert_eq!(config.matches("least_conn;").count(), 2);
        assert_eq!(config.matches('{').count(), config.matches('}').count());
    }

    #[test]
    fn write_if_changed_only_reports_a_real_change() {
        let path = std::env::temp_dir().join(format!(
            "rezure-test-write-if-changed-{}.conf",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);

        assert!(write_if_changed(&path, "a").unwrap());
        assert!(!write_if_changed(&path, "a").unwrap());
        assert!(write_if_changed(&path, "b").unwrap());
        let _ = fs::remove_file(&path);
    }

    /// End to end from the folder: a CodeIgniter 4 app (the shape that used
    /// to be detected as Node) gets a vhost rooted at its `public\`.
    #[test]
    fn vhost_config_roots_codeigniter4_projects_at_public() {
        let dir = std::env::temp_dir().join(format!("rezure-test-vhost-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("public")).unwrap();
        fs::write(dir.join("spark"), "").unwrap();
        fs::write(
            dir.join("package.json"),
            r#"{"dependencies":{"sass":"latest"}}"#,
        )
        .unwrap();

        let serving = crate::services::frameworks::builtin_registry().serving(&dir, None);
        let config = vhost_config(
            "app.test",
            &serving.docroot,
            Path::new("fastcgi_params"),
            &php_pool::upstream_name(php_pool::DEFAULT_BASE_PORT),
        );
        assert!(
            config.contains(&format!("root \"{}\";", conf_path(&dir.join("public")))),
            "{config}"
        );

        fs::remove_dir_all(&dir).unwrap();
    }
}
