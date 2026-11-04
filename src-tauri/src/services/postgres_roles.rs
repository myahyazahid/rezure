//! Users on Rezure's own PostgreSQL — login roles an app connects as, and
//! the settings snippets that point an app at one.
//!
//! The server Rezure bootstraps has only superusers (`postgres`, `root`),
//! which is fine for a Laravel project but not for everything: Odoo refuses
//! to run as `postgres` outright, and Django, Rails or any app that will one
//! day meet a real server is better developed against a user of its own that
//! owns its own database. This adds exactly that and no more — `LOGIN`,
//! optionally `CREATEDB`, optionally a database it owns. Fine-grained grants
//! are a SQL client's job.
//!
//! Always against Rezure's own server (`connections::managed_postgres`),
//! never a saved connection to somebody else's: creating users there is not
//! a local development decision.
//!
//! Passwords travel to `psql` on stdin with the rest of the statement (see
//! `postgres_client`'s module docs) and are never stored by Rezure. The
//! server authenticates local connections with `trust`, so a password only
//! matters once the app's settings are taken to another server — which is
//! why one is still worth setting.

use serde::{Deserialize, Serialize};

use super::database;
use super::postgres_client::{quote_ident, quote_literal, Client};
use super::{connections, postgres};
use crate::utils::error::AppError;

/// Roles Rezure's server is bootstrapped with — never dropped or changed
/// here, since the Databases page and Laravel's defaults connect as them.
const BUILTIN_ROLES: [&str; 2] = [postgres::SUPERUSER, postgres::APP_ROLE];

/// One user, as the Users dialog lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostgresRole {
    pub name: String,
    pub can_login: bool,
    pub can_create_db: bool,
    pub superuser: bool,
    /// `postgres` and `root` — shown, but not editable.
    pub builtin: bool,
    /// Databases it owns, by name. Dropping it is refused while there are
    /// any, so the dialog can say which.
    pub owned_databases: Vec<String>,
}

/// A user to create.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewRole {
    pub name: String,
    /// `None` or empty: no password.
    pub password: Option<String>,
    /// Lets it create databases of its own — what Odoo and test runners
    /// (Django's, Rails') need.
    pub can_create_db: bool,
    /// A database to create, owned by the new user.
    pub database: Option<String>,
}

fn client() -> Result<Client, AppError> {
    let connection = connections::managed_postgres()
        .ok_or_else(|| AppError::BinaryNotInstalled(postgres::SERVICE_NAME.to_string()))?;
    Client::for_connection(connection)
}

/// The name rules for a user: those for a database name, minus the `pg_`
/// prefix PostgreSQL keeps for itself, and never one of the built-in two.
fn validate_role_name(name: &str) -> Result<(), AppError> {
    database::validate_identifier(name, "user")?;
    if name.to_lowercase().starts_with("pg_") {
        return Err(AppError::DatabaseQueryFailed(format!(
            "\"{name}\" starts with pg_, which PostgreSQL keeps for its own roles - pick another name"
        )));
    }
    if is_builtin(name) {
        return Err(AppError::DatabaseQueryFailed(format!(
            "\"{name}\" is one of the users Rezure's PostgreSQL comes with, and can't be changed here"
        )));
    }
    Ok(())
}

fn is_builtin(name: &str) -> bool {
    BUILTIN_ROLES.contains(&name)
}

/// `PASSWORD '…'`, or `PASSWORD NULL` for none.
fn password_clause(password: Option<&str>) -> String {
    match password.filter(|p| !p.is_empty()) {
        Some(password) => format!("PASSWORD {}", quote_literal(password)),
        None => "PASSWORD NULL".to_string(),
    }
}

fn flag(value: &str) -> bool {
    value.trim() == "t"
}

/// Every user on the server except PostgreSQL's own `pg_*` roles.
pub fn list() -> Result<Vec<PostgresRole>, AppError> {
    let rows = client()?.query(
        postgres::MAINTENANCE_DB,
        "SELECT r.rolname, r.rolcanlogin, r.rolcreatedb, r.rolsuper, \
         coalesce(string_agg(d.datname, ',' ORDER BY d.datname), '') \
         FROM pg_roles r \
         LEFT JOIN pg_database d ON d.datdba = r.oid AND NOT d.datistemplate \
         WHERE r.rolname !~ '^pg_' \
         GROUP BY r.rolname, r.rolcanlogin, r.rolcreatedb, r.rolsuper \
         ORDER BY r.rolname",
    )?;
    Ok(rows
        .into_iter()
        .filter(|row| row.len() >= 5)
        .map(|row| PostgresRole {
            builtin: is_builtin(&row[0]),
            can_login: flag(&row[1]),
            can_create_db: flag(&row[2]),
            superuser: flag(&row[3]),
            owned_databases: row[4]
                .split(',')
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect(),
            name: row[0].clone(),
        })
        .collect())
}

fn exists(client: &Client, name: &str) -> Result<bool, AppError> {
    let rows = client.query(
        postgres::MAINTENANCE_DB,
        &format!(
            "SELECT 1 FROM pg_roles WHERE rolname = {}",
            quote_literal(name)
        ),
    )?;
    Ok(!rows.is_empty())
}

/// Creates a login user, and with `database` a database it owns. All or
/// nothing: a database that can't be created takes the new user back out,
/// rather than leaving half of what was asked for.
pub fn create(new: &NewRole) -> Result<(), AppError> {
    validate_role_name(&new.name)?;
    let database = new.database.as_deref().filter(|d| !d.is_empty());
    if let Some(database) = database {
        database::validate_identifier(database, "database")?;
    }

    let client = client()?;
    if exists(&client, &new.name)? {
        return Err(AppError::DatabaseQueryFailed(format!(
            "a user named \"{}\" already exists",
            new.name
        )));
    }
    client.query(
        postgres::MAINTENANCE_DB,
        &format!(
            "CREATE ROLE {} LOGIN {} {}",
            quote_ident(&new.name),
            if new.can_create_db {
                "CREATEDB"
            } else {
                "NOCREATEDB"
            },
            password_clause(new.password.as_deref())
        ),
    )?;

    if let Some(database) = database {
        let created = client.query(
            postgres::MAINTENANCE_DB,
            &format!(
                "CREATE DATABASE {} OWNER {} ENCODING 'UTF8' TEMPLATE template0",
                quote_ident(database),
                quote_ident(&new.name)
            ),
        );
        if let Err(err) = created {
            let _ = client.query(
                postgres::MAINTENANCE_DB,
                &format!("DROP ROLE {}", quote_ident(&new.name)),
            );
            return Err(err);
        }
    }
    log::info!("created PostgreSQL user {}", new.name);
    Ok(())
}

/// Creates `name` if it's missing, otherwise brings it in line: a login
/// with `password`, and `CREATEDB` when asked for. What an app's one-click
/// setup (`services::odoo`) calls, where running it twice must be harmless.
pub fn ensure(name: &str, password: Option<&str>, can_create_db: bool) -> Result<(), AppError> {
    validate_role_name(name)?;
    let client = client()?;
    let verb = if exists(&client, name)? {
        "ALTER"
    } else {
        "CREATE"
    };
    client
        .query(
            postgres::MAINTENANCE_DB,
            &format!(
                "{verb} ROLE {} LOGIN {} {}",
                quote_ident(name),
                if can_create_db {
                    "CREATEDB"
                } else {
                    "NOCREATEDB"
                },
                password_clause(password)
            ),
        )
        .map(|_| ())
}

/// Sets, or with `None` clears, a user's password.
pub fn set_password(name: &str, password: Option<&str>) -> Result<(), AppError> {
    validate_role_name(name)?;
    client()?
        .query(
            postgres::MAINTENANCE_DB,
            &format!(
                "ALTER ROLE {} {}",
                quote_ident(name),
                password_clause(password)
            ),
        )
        .map(|_| ())
}

/// Drops a user. Refused while it owns a database, naming which — dropping
/// those along with it is a decision for the Databases page, where deleting
/// a database is confirmed on its own.
pub fn drop(name: &str) -> Result<(), AppError> {
    validate_role_name(name)?;
    let owned = list()?
        .into_iter()
        .find(|role| role.name == name)
        .map(|role| role.owned_databases)
        .unwrap_or_default();
    if !owned.is_empty() {
        return Err(AppError::DatabaseQueryFailed(format!(
            "\"{name}\" still owns the database{} {} - delete {} on the Databases page first, then the user",
            if owned.len() == 1 { "" } else { "s" },
            owned.join(", "),
            if owned.len() == 1 { "it" } else { "them" }
        )));
    }
    client()?
        .query(
            postgres::MAINTENANCE_DB,
            &format!("DROP ROLE {}", quote_ident(name)),
        )
        .map(|_| ())
}

/// One way to write an app's database settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSnippet {
    pub id: &'static str,
    /// What it's for: "Laravel (.env)", "Django / Node (DATABASE_URL)".
    pub label: &'static str,
    pub text: String,
}

/// `%`-encodes everything but URL-safe characters, for a user or password
/// inside a `postgresql://` URL.
fn url_encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A `.env` value: bare when it's plain, double-quoted otherwise.
fn env_value(text: &str) -> String {
    if !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        text.to_string()
    } else {
        format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// A single-quoted YAML scalar.
fn yaml_value(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The settings that point an app at `user` on Rezure's PostgreSQL, in the
/// forms the common frameworks read. `password` is what the user just set —
/// Rezure doesn't keep it — so without one the snippets leave it empty.
/// `database` defaults to one named after the user.
pub fn snippets(
    user: &str,
    password: Option<&str>,
    database: Option<&str>,
) -> Vec<ConnectionSnippet> {
    let password = password.unwrap_or_default();
    let database = database.filter(|d| !d.is_empty()).unwrap_or(user);
    let host = postgres::HOST;
    let port = postgres::PORT;
    let auth = if password.is_empty() {
        url_encode(user)
    } else {
        format!("{}:{}", url_encode(user), url_encode(password))
    };
    let url = format!("postgresql://{auth}@{host}:{port}/{}", url_encode(database));

    vec![
        ConnectionSnippet {
            id: "url",
            label: "Connection URL",
            text: url.clone(),
        },
        ConnectionSnippet {
            id: "laravel",
            label: "Laravel (.env)",
            text: format!(
                "DB_CONNECTION=pgsql\nDB_HOST={host}\nDB_PORT={port}\nDB_DATABASE={}\nDB_USERNAME={}\nDB_PASSWORD={}",
                env_value(database),
                env_value(user),
                if password.is_empty() { String::new() } else { env_value(password) }
            ),
        },
        ConnectionSnippet {
            id: "database-url",
            label: "Django / Node / Prisma (.env)",
            text: format!("DATABASE_URL={url}"),
        },
        ConnectionSnippet {
            id: "rails",
            label: "Rails (config/database.yml)",
            text: format!(
                "development:\n  adapter: postgresql\n  encoding: unicode\n  host: {host}\n  port: {port}\n  database: {}\n  username: {}\n  password: {}",
                yaml_value(database),
                yaml_value(user),
                yaml_value(password)
            ),
        },
        ConnectionSnippet {
            id: "jdbc",
            label: "JDBC (Spring, DBeaver)",
            text: format!(
                "jdbc:postgresql://{host}:{port}/{}?user={}{}",
                url_encode(database),
                url_encode(user),
                if password.is_empty() {
                    String::new()
                } else {
                    format!("&password={}", url_encode(password))
                }
            ),
        },
        ConnectionSnippet {
            id: "odoo",
            label: "Odoo (odoo.conf)",
            text: format!(
                "db_host = {host}\ndb_port = {port}\ndb_user = {user}\ndb_password = {}\n{}",
                if password.is_empty() { "False" } else { password },
                postgres::active_tool(postgres::CLIENT_EXE)
                    .ok()
                    .and_then(|psql| psql.parent().map(|bin| format!("pg_path = {}", bin.display())))
                    .unwrap_or_default()
            )
            .trim_end()
            .to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgresql_own_and_rezures_own_roles_are_refused() {
        assert!(validate_role_name("pg_monitor").is_err());
        assert!(validate_role_name("PG_x").is_err());
        assert!(validate_role_name("postgres").is_err());
        assert!(validate_role_name("root").is_err());
        assert!(validate_role_name("odoo").is_ok());
        assert!(validate_role_name("my-app_1").is_ok());
        // Nothing that could close the quoted identifier.
        assert!(validate_role_name("a\"; DROP ROLE x; --").is_err());
        assert!(validate_role_name("").is_err());
    }

    #[test]
    fn a_password_is_quoted_and_an_empty_one_means_none() {
        assert_eq!(password_clause(Some("it's")), "PASSWORD 'it''s'");
        assert_eq!(password_clause(Some("")), "PASSWORD NULL");
        assert_eq!(password_clause(None), "PASSWORD NULL");
    }

    #[test]
    fn snippets_escape_each_format_its_own_way() {
        let all = snippets("shop", Some("p@ss:w/rd \"x\""), Some("shop_db"));
        let text = |id: &str| all.iter().find(|s| s.id == id).unwrap().text.clone();

        assert_eq!(
            text("url"),
            "postgresql://shop:p%40ss%3Aw%2Frd%20%22x%22@127.0.0.1:5432/shop_db"
        );
        assert!(text("laravel").contains("DB_PASSWORD=\"p@ss:w/rd \\\"x\\\"\""));
        assert!(text("laravel").contains("DB_DATABASE=shop_db"));
        assert!(text("database-url").starts_with("DATABASE_URL=postgresql://shop:"));
        assert!(text("rails").contains("password: 'p@ss:w/rd \"x\"'"));
        assert!(text("jdbc").contains("?user=shop&password=p%40ss"));
        assert!(text("odoo").contains("db_user = shop"));
    }

    #[test]
    fn without_a_password_or_database_the_snippets_still_connect() {
        let all = snippets("odoo", None, None);
        let text = |id: &str| all.iter().find(|s| s.id == id).unwrap().text.clone();
        assert_eq!(text("url"), "postgresql://odoo@127.0.0.1:5432/odoo");
        assert!(text("laravel").ends_with("DB_PASSWORD="));
        assert!(text("odoo").contains("db_password = False"));
    }

    /// Against the real PostgreSQL service, which has to be running. Run with
    /// `cargo test --lib services::postgres_roles::tests::round_trip_against_local_postgres -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn round_trip_against_local_postgres() {
        let name = "rezure_role_test";
        let db = "rezure_role_test_db";
        let _ = super::super::postgres_client::Client::for_connection(
            connections::managed_postgres().unwrap(),
        )
        .and_then(|c| {
            c.query(
                postgres::MAINTENANCE_DB,
                &format!("DROP DATABASE IF EXISTS {db}"),
            )
        });
        let _ = drop(name);

        create(&NewRole {
            name: name.to_string(),
            password: Some("secret".to_string()),
            can_create_db: true,
            database: Some(db.to_string()),
        })
        .unwrap();
        let role = list()
            .unwrap()
            .into_iter()
            .find(|r| r.name == name)
            .unwrap();
        println!("{role:?}");
        assert!(role.can_login && role.can_create_db && !role.superuser);
        assert_eq!(role.owned_databases, [db]);

        // Owns a database: refused, and says which.
        let err = drop(name).unwrap_err().to_string();
        assert!(err.contains(db), "{err}");

        ensure(name, Some("other"), false).unwrap();
        let role = list()
            .unwrap()
            .into_iter()
            .find(|r| r.name == name)
            .unwrap();
        assert!(!role.can_create_db);

        super::super::postgres_client::Client::for_connection(
            connections::managed_postgres().unwrap(),
        )
        .unwrap()
        .query(postgres::MAINTENANCE_DB, &format!("DROP DATABASE {db}"))
        .unwrap();
        drop(name).unwrap();
        assert!(list().unwrap().iter().all(|r| r.name != name));
    }
}
