//! Moves an older install's data into the single [`paths::home`] root.
//!
//! Rezure used to spread its storage over three OS locations. Changing where
//! it looks without moving what's already there would present as data loss:
//! the project list empties, linked projects disappear, and MariaDB bootstraps
//! a fresh datadir over the top of databases that are still on disk.
//!
//! So this runs once at startup, before anything reads a path.
//!
//! # Rules it follows
//!
//! * **Never overwrite.** A destination that already exists is left alone and
//!   the source is kept, not merged. Merging two datadirs is not a thing that
//!   can be done safely, and a half-merge is worse than either half.
//! * **Never delete on failure.** Every step is a rename, falling back to
//!   copy-then-remove across volumes. If the copy fails the original stays
//!   where it is.
//! * **Best effort.** A step that can't complete logs and is skipped. Refusing
//!   to start because one folder wouldn't move would strand the user with no
//!   way to reach the app at all.

use std::fs;
use std::path::{Path, PathBuf};

use crate::utils::paths;

/// One old location and where it belongs now.
struct Move {
    what: &'static str,
    from: PathBuf,
    to: PathBuf,
}

/// The pre-1.0 layout, in the three places it used to live.
fn planned() -> Vec<Move> {
    let mut moves = Vec::new();

    let local = dirs::data_local_dir().map(|base| base.join("Rezure"));
    let roaming = dirs::config_dir().map(|base| base.join("Rezure"));
    let home = dirs::home_dir().map(|base| base.join("rezure"));

    let Ok(root) = paths::home() else {
        return moves;
    };

    // Nothing to do when the old local root *is* the new one — that's the
    // fallback case on a machine where C:\ wasn't usable.
    if let Some(local) = local.filter(|old| old != &root) {
        moves.push(Move {
            what: "runtimes",
            from: local.join("bin"),
            to: root.join("bin"),
        });
        moves.push(Move {
            what: "service data",
            from: local.join("data"),
            to: root.join("data"),
        });
        // `current` is deliberately *not* moved. It holds a junction, and a
        // junction stores an absolute target — carrying it over would leave it
        // pointing into the old `bin`, which this migration has just emptied,
        // so `php` on PATH would resolve to nothing. It is dropped instead and
        // rebuilt by `php_path::sync` once the new layout is in place.
        moves.push(Move {
            what: "the database file",
            from: local.join("rezure.db"),
            to: root.join("rezure.db"),
        });
    }

    if let Some(roaming) = roaming {
        for file in ["settings.json", "profiles.json", "links.json"] {
            moves.push(Move {
                what: "config",
                from: roaming.join(file),
                to: root.join("etc").join(file),
            });
        }
    }

    if let Some(home) = home.filter(|old| old != &root) {
        moves.push(Move {
            what: "projects",
            from: home.join("www"),
            to: root.join("www"),
        });
        moves.push(Move {
            what: "dumps",
            from: home.join("dumps"),
            to: root.join("dumps"),
        });
        // The old drop-in `bin` becomes `custom`, so it can't collide with the
        // managed `bin` now that both live under one root.
        moves.push(Move {
            what: "hand-added runtimes",
            from: home.join("bin"),
            to: root.join("custom"),
        });
    }

    moves
}

/// `rename` first — it's atomic and instant within a volume. Across volumes
/// (an old `%LOCALAPPDATA%` on C: and a root the user pointed at D:) Windows
/// refuses, so fall back to copying and only remove the source once the copy
/// has fully succeeded.
fn relocate(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    if from.is_dir() {
        copy_dir(from, to)?;
        fs::remove_dir_all(from)
    } else {
        fs::copy(from, to)?;
        fs::remove_file(from)
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        // Junctions are followed rather than recreated: the only one Rezure
        // makes is `current\php`, which is rebuilt on the next switch anyway.
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Deletes a stale PHP junction, in the old location and the new one.
///
/// Removed with `Directory::delete` rather than `remove_dir_all`, which
/// follows a reparse point and would take the PHP install it points at with
/// it. Losing the link costs nothing — `php_path::sync` rebuilds it — but
/// losing the target would mean re-downloading a runtime.
fn drop_stale_php_junctions() {
    let mut candidates = Vec::new();
    if let Some(local) = dirs::data_local_dir() {
        candidates.push(local.join("Rezure").join("current"));
    }
    if let Ok(root) = paths::home() {
        candidates.push(root.join("current"));
    }

    for current in candidates {
        let link = current.join("php");
        let Ok(meta) = fs::symlink_metadata(&link) else {
            continue;
        };
        if !meta.file_type().is_symlink() && !meta.is_dir() {
            continue;
        }
        match fs::remove_dir(&link) {
            Ok(()) => log::info!(
                "migration: dropped the stale PHP link at {}",
                link.display()
            ),
            Err(err) => log::warn!(
                "migration: could not drop the PHP link at {}: {err}",
                link.display()
            ),
        }
        let _ = fs::remove_dir(&current);
    }
}

/// Rewrites a recorded path that pointed into one of the old roots.
///
/// Reuses [`planned`] as the single description of what moved where, so a
/// mapping can never be added for the files and forgotten for the paths that
/// name them. Returns `None` when `recorded` is somewhere else entirely —
/// an adopted Laragon or XAMPP datadir, most importantly, which this must
/// never touch.
fn rewrite_recorded(recorded: &str) -> Option<String> {
    let candidate = Path::new(recorded);
    for step in planned() {
        if let Ok(rest) = candidate.strip_prefix(&step.from) {
            return Some(step.to.join(rest).display().to_string());
        }
    }
    None
}

/// Repoints paths stored *inside* the config at where their files now are.
///
/// Moving the files is only half the job. A database profile records its
/// datadir as an absolute path, so after the move the active profile still
/// names a directory that no longer exists — and `needs_bootstrap` reads a
/// missing datadir as "brand new" and initialises an empty one over the top.
/// The databases are all still on disk, and the app shows none of them.
///
/// Returns how many profiles were rewritten.
fn repair_recorded_paths() -> usize {
    let mut store = crate::config::profiles::load();
    let mut changed = 0;

    for profile in &mut store.profiles {
        let mut touched = false;

        if let Some(fixed) = rewrite_recorded(&profile.datadir_path) {
            log::info!(
                "migration: profile \"{}\" datadir {} -> {fixed}",
                profile.name,
                profile.datadir_path
            );
            profile.datadir_path = fixed;
            touched = true;
        }
        if let Some(dir) = &profile.binary_dir {
            if let Some(fixed) = rewrite_recorded(dir) {
                profile.binary_dir = Some(fixed);
                touched = true;
            }
        }
        if let Some(file) = &profile.defaults_file {
            if let Some(fixed) = rewrite_recorded(file) {
                profile.defaults_file = Some(fixed);
                touched = true;
            }
        }

        if touched {
            changed += 1;
        }
    }

    if changed > 0 {
        if let Err(err) = crate::config::profiles::save(&store) {
            log::warn!("migration: could not persist repaired profile paths: {err}");
            return 0;
        }
    }
    changed
}

/// Marks that the one-time startup repairs have been done.
///
/// They cost three PowerShell spawns — reading the user PATH, and reading the
/// junction's target — which is real startup latency to pay on every launch
/// for a repair that only applies once, to an install that predates the
/// single-folder layout. The marker bounds that to the first launch after
/// upgrading.
fn repair_marker() -> Option<PathBuf> {
    paths::etc().ok().map(|etc| etc.join(".layout-repaired"))
}

/// Whether the one-time PATH and junction repairs still need to run.
///
/// `is_some_and` rather than `is_none_or`: the latter is only stable from Rust
/// 1.82 and this crate's MSRV is 1.77.2.
pub fn needs_startup_repairs() -> bool {
    !repair_marker().is_some_and(|marker| marker.exists())
}

/// Records that they completed. Only call after they actually succeeded — a
/// failed repair that marked itself done would never be retried.
pub fn mark_startup_repairs_done() {
    let Some(marker) = repair_marker() else {
        return;
    };
    if let Err(err) = fs::write(
        &marker,
        b"Rezure has repaired this install's paths for the single-folder layout.\n",
    ) {
        log::warn!(
            "could not write {}: the repairs will run again next launch: {err}",
            marker.display()
        );
    }
}

/// Runs the migration. Returns how many locations were moved.
pub fn run() -> usize {
    let mut moved = 0;

    drop_stale_php_junctions();

    for step in planned() {
        if !step.from.exists() {
            continue;
        }
        if step.to.exists() {
            log::info!(
                "migration: {} already present at {}, leaving {} alone",
                step.what,
                step.to.display(),
                step.from.display()
            );
            continue;
        }
        match relocate(&step.from, &step.to) {
            Ok(()) => {
                log::info!("migration: moved {} to {}", step.what, step.to.display());
                moved += 1;
            }
            Err(err) => log::warn!(
                "migration: could not move {} from {}: {err} — it stays where it is",
                step.what,
                step.from.display()
            ),
        }
    }

    // Always, not only when something moved: an earlier launch may have moved
    // the files and left the recorded paths behind, which is the state this
    // exists to repair.
    let repointed = repair_recorded_paths();
    if repointed > 0 {
        log::info!("migration: repointed {repointed} profile path(s)");
    }

    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_adopted_datadir_outside_the_old_roots_is_never_rewritten() {
        // The one that must never break: a Laragon or XAMPP profile points at
        // data Rezure does not own. Rewriting it would aim the server at a
        // directory that doesn't exist, and bootstrapping would then create an
        // empty one — losing sight of somebody else's databases entirely.
        for foreign in [
            r"C:\laragon\data\mysql-8.4",
            r"C:\xampp\mysql\data",
            r"D:\somewhere\else",
        ] {
            assert_eq!(
                rewrite_recorded(foreign),
                None,
                "{foreign} must be left as-is"
            );
        }
    }

    #[test]
    fn a_datadir_under_an_old_root_is_repointed_at_the_new_one() {
        let Some(local) = dirs::data_local_dir() else {
            return;
        };
        let root = paths::home().expect("a root must resolve");
        if local.join("Rezure") == root {
            return; // the fallback layout: nothing moved, nothing to rewrite
        }

        let old = local
            .join("Rezure")
            .join("data")
            .join("mariadb")
            .join("data");
        let expected = root.join("data").join("mariadb").join("data");
        assert_eq!(
            rewrite_recorded(&old.display().to_string()),
            Some(expected.display().to_string())
        );
    }

    #[test]
    fn nothing_is_planned_onto_itself() {
        // A move whose source equals its destination would delete the data it
        // was meant to preserve.
        for step in planned() {
            assert_ne!(step.from, step.to, "{} would move onto itself", step.what);
        }
    }

    #[test]
    fn every_destination_is_inside_the_root() {
        let root = paths::home().expect("a root must resolve");
        for step in planned() {
            assert!(
                step.to.starts_with(&root),
                "{} would land outside the root at {}",
                step.what,
                step.to.display()
            );
        }
    }

    /// Runs the real startup sequence against this machine and checks that it
    /// leaves nothing dangling. Ignored because it rewrites the user's PATH
    /// and rebuilds the PHP junction.
    ///
    /// `cargo test --lib utils::migrate::tests::startup_repairs_leave_nothing_dangling -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn startup_repairs_leave_nothing_dangling() {
        use crate::services::php_path;

        let moved = run();
        println!("moved {moved} location(s)");

        let repaired = php_path::repair_legacy_entry().expect("PATH must be readable");
        println!("stale PATH entry repaired: {repaired}");

        let status = php_path::status().expect("status must be readable");
        println!(
            "on_path={} in_sync={} target={:?}",
            status.on_path, status.in_sync, status.target
        );
        if status.on_path && !status.in_sync {
            php_path::sync().expect("the junction must be rebuildable");
        }

        let after = php_path::status().expect("status must be re-readable");
        if after.on_path {
            assert!(
                after.in_sync,
                "a repaired link has to point at the active version"
            );
            let target = after.target.expect("an on-PATH link must have a target");
            assert!(
                Path::new(&target).join("php.exe").is_file(),
                "the link resolves to {target}, which holds no php.exe"
            );
        }

        // Each PHP folder's own `php.ini` holds an absolute `extension_dir`,
        // so the move left them naming a folder that is no longer there — and
        // PHP responds by loading no extensions at all.
        for runtime in crate::services::php::installed() {
            let repaired = crate::services::php_ini::repair_extension_dir(&runtime.dir)
                .expect("php.ini must be repairable");
            println!("php {} ini repaired={repaired}", runtime.version);

            let ini = runtime.dir.join("php.ini");
            if let Ok(content) = std::fs::read_to_string(&ini) {
                for line in content.lines() {
                    let trimmed = line.trim_start();
                    if trimmed.starts_with(';') || !trimmed.starts_with("extension_dir") {
                        continue;
                    }
                    let value = trimmed
                        .split_once('=')
                        .map(|(_, v)| v.trim().trim_matches('"').to_string())
                        .unwrap_or_default();
                    // An `ext` folder can legitimately be absent from a build
                    // that ships no extensions; what must never happen is the
                    // ini naming a directory that isn't there.
                    if runtime.dir.join("ext").is_dir() {
                        assert!(
                            Path::new(&value).is_dir(),
                            "php {} names an extension_dir that does not exist: {value}",
                            runtime.version
                        );
                    }
                }
            }
        }

        // The failure this whole module exists to prevent: a profile naming a
        // datadir that isn't there reads as "brand new" to `needs_bootstrap`,
        // which then initialises an empty one over the top.
        for profile in crate::config::profiles::load().profiles {
            let exists = Path::new(&profile.datadir_path).is_dir();
            println!(
                "profile {:<16} datadir {} exists={exists}",
                profile.name, profile.datadir_path
            );
            assert!(
                exists,
                "profile \"{}\" points at a datadir that does not exist: {}",
                profile.name, profile.datadir_path
            );
        }
    }

    #[test]
    fn relocate_moves_a_tree_and_leaves_nothing_behind() {
        let base = std::env::temp_dir().join(format!("rezure-migrate-{}", std::process::id()));
        let from = base.join("from");
        let to = base.join("to");
        let _ = fs::remove_dir_all(&base);

        fs::create_dir_all(from.join("nested")).unwrap();
        fs::write(from.join("nested").join("a.txt"), b"hello").unwrap();

        relocate(&from, &to).unwrap();

        assert!(!from.exists(), "the source should be gone");
        assert_eq!(
            fs::read_to_string(to.join("nested").join("a.txt")).unwrap(),
            "hello"
        );

        let _ = fs::remove_dir_all(&base);
    }
}
