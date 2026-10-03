//! A TCP door to Rezure's LocalDB instance, for SQL clients that can't open
//! a named pipe.
//!
//! LocalDB listens on a named pipe only — `\\.\pipe\LOCALDB#…\tsql\query`,
//! renamed every time the instance starts — and opens no TCP port at all.
//! That's fine for PHP and SSMS, which go through Microsoft's own client
//! libraries, but TablePlus, DBeaver and HeidiSQL only speak to SQL Server
//! over TCP and can't reach it.
//!
//! So while Rezure runs, `127.0.0.1:`[`PORT`] is relayed byte for byte to
//! the instance's current pipe. TDS — SQL Server's protocol — is the same
//! stream over either transport, so nothing in between has to understand
//! it; this was checked against the real instance with a 300,000-character
//! value and 100,000 streamed rows before being written here.
//!
//! Each new TCP connection resolves the pipe afresh, starting the instance
//! if it has stopped — LocalDB shuts itself down when idle, and its pipe
//! name changes when it comes back. That keeps the TCP side behaving like
//! the pipe does: always there, starting the server on demand.
//!
//! # What a client has to know
//!
//! * **No encryption.** Over its pipe LocalDB offers no TLS, so its
//!   pre-login answer is "encryption not supported". A client that insists
//!   on TLS (ODBC Driver 18's default, newer JDBC drivers) has to be told
//!   not to, or it refuses to connect.
//! * **A login.** Windows Authentication works through the bridge, but
//!   TablePlus can't do it — hence the passwordless `rezure` SQL login (see
//!   `mssql::ensure_localdb_login`), the same stance as MariaDB's `root`.
//!
//! Bound to `127.0.0.1` only, like every other service Rezure runs.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};
use tokio::net::{TcpListener, TcpStream};

use super::mssql_localdb;

/// Not 1433: a SQL Server Express or Developer edition the user installs
/// later would want that one, and a Rezure that quietly held it would stop
/// that server from starting.
pub const PORT: u16 = 14330;

/// Windows' `ERROR_PIPE_BUSY` — every instance of the pipe is taken for the
/// moment; retrying shortly is the documented answer.
const ERROR_PIPE_BUSY: i32 = 231;

static STARTED: AtomicBool = AtomicBool::new(false);
static LISTENING: AtomicBool = AtomicBool::new(false);

/// The port, while the bridge is actually listening on it.
pub fn listening_port() -> Option<u16> {
    LISTENING.load(Ordering::SeqCst).then_some(PORT)
}

/// Starts the bridge if LocalDB is installed and it isn't running yet.
/// Called at startup and right after LocalDB is installed.
///
/// A port that's taken is logged, not fatal: LocalDB itself is unaffected —
/// only handing off to a TCP-only client stops working, and the service
/// card stops showing the port.
pub fn start() {
    if !mssql_localdb::is_installed() || STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async {
        let listener = match TcpListener::bind(("127.0.0.1", PORT)).await {
            Ok(listener) => listener,
            Err(err) => {
                log::warn!("LocalDB bridge can't listen on 127.0.0.1:{PORT}: {err}");
                STARTED.store(false, Ordering::SeqCst);
                return;
            }
        };
        LISTENING.store(true, Ordering::SeqCst);
        log::info!("LocalDB is reachable on 127.0.0.1:{PORT}");
        loop {
            match listener.accept().await {
                Ok((socket, _)) => {
                    tauri::async_runtime::spawn(relay(socket));
                }
                Err(err) => log::warn!("LocalDB bridge accept failed: {err}"),
            }
        }
    });
}

/// Carries one client's connection to the instance and back, until either
/// side hangs up.
async fn relay(mut socket: TcpStream) {
    let pipe = match tokio::task::spawn_blocking(mssql_localdb::pipe_path).await {
        Ok(Ok(pipe)) => pipe,
        Ok(Err(err)) => {
            log::warn!("LocalDB bridge: {err}");
            return;
        }
        Err(err) => {
            log::warn!("LocalDB bridge: pipe lookup panicked: {err}");
            return;
        }
    };
    let mut upstream = match open_pipe(&pipe).await {
        Ok(upstream) => upstream,
        Err(err) => {
            log::warn!("LocalDB bridge can't open {pipe}: {err}");
            return;
        }
    };
    // Either side closing ends both; an error here is just a client going
    // away mid-session, which is not worth more than this.
    let _ = tokio::io::copy_bidirectional(&mut socket, &mut upstream).await;
}

/// Opens the pipe, waiting out a moment where every instance of it is busy.
async fn open_pipe(path: &str) -> std::io::Result<NamedPipeClient> {
    let mut attempts = 0;
    loop {
        match ClientOptions::new().open(path) {
            Ok(client) => return Ok(client),
            Err(err) if err.raw_os_error() == Some(ERROR_PIPE_BUSY) && attempts < 40 => {
                attempts += 1;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(err) => return Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_reported_before_the_bridge_listens() {
        assert_eq!(listening_port(), None);
    }

    #[test]
    fn the_bridge_stays_off_the_port_sql_server_itself_defaults_to() {
        assert_ne!(PORT, 1433);
    }
}
