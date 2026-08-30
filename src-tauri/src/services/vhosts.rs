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
//! `php-cgi -b 127.0.0.1:PHP_FASTCGI_PORT` (see `services::process`) — the
//! standard substitute FastCGI responder on this platform.

use std::fs;
use std::path::{Path, PathBuf};

use super::binaries;
use super::projects::{docroot, scan_projects};
use crate::utils::error::AppError;

/// Must match the port `ProcessService::php()` binds `php-cgi` to.
pub const PHP_FASTCGI_PORT: u16 = 9000;

/// `%LOCALAPPDATA%\Rezure\data\nginx` — Rezure's own generated nginx
/// config, logs, and temp directories (separate from the pristine
/// extracted binary under `binaries::install_root()`).
pub fn nginx_runtime_dir() -> Result<PathBuf, AppError> {
    let base = dirs::data_local_dir().ok_or_else(|| {
        AppError::Io("could not resolve the local app data directory".to_string())
    })?;
    Ok(base.join("Rezure").join("data").join("nginx"))
}

pub fn vhosts_dir() -> Result<PathBuf, AppError> {
    Ok(nginx_runtime_dir()?.join("vhosts"))
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

    # Catches any request whose Host header doesn't match a vhost below.
    server {{
        listen 80 default_server;
        server_name _;
        return 404;
    }}

    include "{vhosts_glob}";
}}
"#,
        error_log = conf_path(&runtime.join("logs").join("error.log")),
        pid_file = conf_path(&runtime.join("nginx.pid")),
        mime_types = conf_path(&mime_types),
        client_body_temp = conf_path(&runtime.join("temp").join("client_body")),
        proxy_temp = conf_path(&runtime.join("temp").join("proxy")),
        fastcgi_temp = conf_path(&runtime.join("temp").join("fastcgi")),
        vhosts_glob = conf_path(&vhosts.join("*.conf")),
    );

    let config_path = runtime.join("nginx.conf");
    fs::write(&config_path, config)
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", config_path.display())))?;
    Ok(config_path)
}

fn vhost_config(domain: &str, project_root: &Path, stack: &str, fastcgi_params: &Path) -> String {
    format!(
        r#"server {{
    listen 80;
    server_name {domain};
    root "{root}";
    index index.php index.html;

    location / {{
        try_files $uri $uri/ /index.php?$query_string;
    }}

    location ~ \.php$ {{
        fastcgi_pass 127.0.0.1:{port};
        fastcgi_index index.php;
        fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;
        include "{fastcgi_params}";
    }}

    location ~ /\.ht {{
        deny all;
    }}
}}
"#,
        domain = domain,
        root = conf_path(&docroot(project_root, stack)),
        port = PHP_FASTCGI_PORT,
        fastcgi_params = conf_path(fastcgi_params),
    )
}

/// Rewrites every project's vhost to match the current scan of
/// `projects::www_root()`, deleting `.conf` files for projects that no
/// longer exist. Returns how many vhosts are active. Nginx only picks up
/// *new* files on its next start (no live reload yet), but existing ones
/// are always kept current.
pub fn sync_vhosts() -> Result<usize, AppError> {
    let vhosts = vhosts_dir()?;
    ensure_dir(&vhosts)?;
    let fastcgi_params = nginx_conf_dir()?.join("fastcgi_params");

    let current = scan_projects()?;
    let current_ids: std::collections::HashSet<&str> =
        current.iter().map(|p| p.id.as_str()).collect();

    // Drop vhosts for projects that no longer exist on disk.
    if let Ok(entries) = fs::read_dir(&vhosts) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_stale = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|stem| !current_ids.contains(stem))
                .unwrap_or(false);
            if is_stale && path.extension().is_some_and(|ext| ext == "conf") {
                let _ = fs::remove_file(&path);
            }
        }
    }

    for project in &current {
        let config = vhost_config(
            &project.domain,
            Path::new(&project.path),
            &project.stack,
            &fastcgi_params,
        );
        let path = vhosts.join(format!("{}.conf", project.id));
        fs::write(&path, config)
            .map_err(|e| AppError::Io(format!("could not write {}: {e}", path.display())))?;
    }

    Ok(current.len())
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
    fn vhost_config_embeds_domain_root_and_fastcgi_port() {
        let config = vhost_config(
            "blog.test",
            Path::new(r"C:\Users\dev\rezure\www\blog"),
            "PHP",
            Path::new(r"C:\nginx\conf\fastcgi_params"),
        );

        assert!(config.contains("server_name blog.test;"));
        assert!(config.contains("root \"C:/Users/dev/rezure/www/blog\";"));
        assert!(config.contains("fastcgi_pass 127.0.0.1:9000;"));
        assert!(config.contains("include \"C:/nginx/conf/fastcgi_params\";"));
    }

    #[test]
    fn vhost_config_roots_laravel_projects_at_public() {
        let dir = std::env::temp_dir().join(format!("rezure-test-vhost-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("public")).unwrap();

        let config = vhost_config("app.test", &dir, "Laravel", Path::new("fastcgi_params"));
        assert!(config.contains(&conf_path(&dir.join("public"))));

        fs::remove_dir_all(&dir).unwrap();
    }
}
