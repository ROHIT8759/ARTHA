//! Bill-image file naming, safe path resolution, and backup/restore support
//! (Phase 10 hardening of Phase 09 backups).
//!
//! Why this is safe to do next to the SQLite backup:
//!
//! * Uploaded photos are **content-addressed and immutable**: the file is written as
//!   `<sha256>.<png|jpg|webp>` *before* its `bill_images` row exists and is never modified
//!   or deleted afterwards. A database snapshot therefore pins exactly which image bytes
//!   it needs, and those bytes cannot change underneath a backup.
//! * A backup copies every image referenced by the snapshot into a shared content-addressed
//!   store (`<backup_dir>/images/`), then writes a small sidecar manifest
//!   (`artha-backup-<id>.images.json`) bound to the database backup's checksum.
//! * Restore verifies the database backup **and** every image against the checksummed
//!   snapshot before changing anything, copies images in additively (never deleting a live
//!   file), and only then restores the database.
//!
//! Names are never trusted: only `<64 hex>.<ext>` file names are accepted, resolved
//! against the configured directories, regular files only (symlinks are rejected).
//! Nothing here deletes an image.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::ApiError;
use crate::ids::new_id;

/// Sub-directory of the backup directory holding the shared copies of bill images.
pub const IMAGE_STORE_DIRNAME: &str = "images";
const MANIFEST_FORMAT_VERSION: u32 = 1;
/// Larger than any real manifest (about 100 bytes per image).
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
const MAX_MANIFEST_IMAGES: usize = 100_000;
/// Upload limit (`routes::purchases::MAX_BILL_IMAGE_BYTES`); a manifest claiming more is bogus.
const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
const ALLOWED_EXTENSIONS: [&str; 3] = ["png", "jpg", "webp"];

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// True for exactly `<64 lowercase hex>.<png|jpg|webp>`: the only names ever written.
pub fn is_valid_image_name(name: &str) -> bool {
    let Some((stem, ext)) = name.split_once('.') else {
        return false;
    };
    stem.len() == 64
        && stem.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        && ALLOWED_EXTENSIONS.contains(&ext)
}

/// The image file name inside a stored `bill_images.path`. Older rows hold an absolute
/// path (possibly from another machine or OS), so only the last component is used and it
/// must be a valid image name; anything else (`..`, separators, odd names) yields `None`.
pub fn image_name_from_path(stored: &str) -> Option<String> {
    let name = stored.rsplit(['/', '\\']).next()?;
    is_valid_image_name(name).then(|| name.to_string())
}

/// Resolves a stored `bill_images.path` against the configured upload directory.
pub fn resolve_upload_path(upload_dir: &Path, stored: &str) -> Option<PathBuf> {
    image_name_from_path(stored).map(|name| upload_dir.join(name))
}

fn internal(context: &str, e: impl std::fmt::Display) -> ApiError {
    ApiError::Internal(format!("{context}: {e}"))
}

fn damaged() -> ApiError {
    ApiError::BadRequest(
        "The bill photos stored with this backup are missing, damaged or inconsistent; refusing to restore. Verify the backup for details."
            .into(),
    )
}

/// Reads a regular, non-symlink file. `None` if missing, a symlink, or not a file.
fn read_regular_file(path: &Path) -> Option<Vec<u8>> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    std::fs::read(path).ok()
}

fn regular_file_len(path: &Path) -> Option<u64> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    meta.is_file().then_some(meta.len())
}

/// Writes `bytes` to `dest` through a temporary file in the same directory, fsyncs, then
/// renames, so a crash never leaves a truncated file under the final name.
pub fn write_file_atomic(dest: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = dest.parent().unwrap_or_else(|| Path::new("."));
    let tmp = dir.join(format!(".tmp-{}", new_id()));
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, dest)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// The images a database refers to.
pub struct Referenced {
    /// file name -> sha256 (the name's stem), deduplicated.
    pub images: BTreeMap<String, String>,
    /// Rows whose stored path/hash do not describe a valid image file name.
    pub invalid_rows: usize,
}

pub fn referenced_images(conn: &Connection) -> Result<Referenced, ApiError> {
    let mut stmt = conn.prepare("SELECT sha256_hex, path FROM bill_images")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut images = BTreeMap::new();
    let mut invalid_rows = 0usize;
    for row in rows {
        let (sha, path) = row?;
        let sha = sha.to_ascii_lowercase();
        match image_name_from_path(&path) {
            Some(name) if name.split_once('.').map(|(stem, _)| stem) == Some(sha.as_str()) => {
                images.insert(name, sha);
            }
            _ => invalid_rows += 1,
        }
    }
    Ok(Referenced {
        images,
        invalid_rows,
    })
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestImage {
    pub name: String,
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format_version: u32,
    pub backup_id: String,
    /// SHA-256 of the database backup file this manifest belongs to.
    pub db_sha256: String,
    /// Images present in the shared store, one entry per referenced image.
    pub images: Vec<ManifestImage>,
    /// Referenced images that could not be stored (file absent or corrupt at backup time).
    pub missing: Vec<String>,
    /// `bill_images` rows that did not describe a valid image file name.
    pub invalid_rows: usize,
}

pub fn manifest_path_for(db_backup_path: &Path) -> PathBuf {
    let mut p = db_backup_path.to_path_buf();
    p.set_extension("images.json");
    p
}

pub fn store_dir_for(db_backup_path: &Path) -> PathBuf {
    db_backup_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(IMAGE_STORE_DIRNAME)
}

/// Counts shown to the owner after a backup.
#[derive(Debug, Clone, Copy, Default)]
pub struct ImageCounts {
    pub stored: usize,
    pub missing: usize,
}

/// Summary of a backup's image set, if it has a readable manifest.
pub fn manifest_counts(db_backup_path: &Path) -> Option<ImageCounts> {
    let mpath = manifest_path_for(db_backup_path);
    let meta = std::fs::symlink_metadata(&mpath).ok()?;
    if !meta.is_file() || meta.len() > MAX_MANIFEST_BYTES {
        return None;
    }
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(&mpath).ok()?).ok()?;
    Some(ImageCounts {
        stored: manifest.images.len(),
        missing: manifest.missing.len() + manifest.invalid_rows,
    })
}

pub fn has_manifest(db_backup_path: &Path) -> bool {
    std::fs::symlink_metadata(manifest_path_for(db_backup_path)).is_ok()
}

/// Backup side: copies every image referenced by the (already written) database backup
/// into the shared store and writes the manifest. An image whose live file is absent or
/// does not match its recorded hash is *reported* in the manifest, never silently dropped.
pub fn snapshot_images(
    upload_dir: &Path,
    db_backup_path: &Path,
    backup_id: &str,
    db_sha256: &str,
) -> Result<ImageCounts, ApiError> {
    let referenced = {
        let conn = Connection::open_with_flags(db_backup_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| internal("failed to open backup for image snapshot", e))?;
        referenced_images(&conn)?
    };

    let store = store_dir_for(db_backup_path);
    std::fs::create_dir_all(&store).map_err(|e| internal("failed to create image store", e))?;

    let mut images = Vec::new();
    let mut missing = Vec::new();
    for (name, sha) in &referenced.images {
        let src = upload_dir.join(name);
        let dst = store.join(name);
        match regular_file_len(&src) {
            Some(size) => {
                // A copy of the same size is trusted here (the files are immutable and
                // content-addressed); full hashing happens on verify and on restore.
                if regular_file_len(&dst) != Some(size) {
                    let bytes = match read_regular_file(&src) {
                        Some(b) if sha256_hex(&b) == *sha => b,
                        _ => {
                            tracing::warn!(image = %name, "bill image missing or corrupt at backup time");
                            missing.push(name.clone());
                            continue;
                        }
                    };
                    write_file_atomic(&dst, &bytes)
                        .map_err(|e| internal("failed to copy bill image into backup", e))?;
                }
                images.push(ManifestImage {
                    name: name.clone(),
                    sha256: sha.clone(),
                    size_bytes: size,
                });
            }
            None => match read_regular_file(&dst) {
                // The live file is gone but an earlier backup still holds a good copy.
                Some(b) if sha256_hex(&b) == *sha => images.push(ManifestImage {
                    name: name.clone(),
                    sha256: sha.clone(),
                    size_bytes: b.len() as u64,
                }),
                _ => {
                    tracing::warn!(image = %name, "bill image missing at backup time");
                    missing.push(name.clone());
                }
            },
        }
    }

    let counts = ImageCounts {
        stored: images.len(),
        missing: missing.len() + referenced.invalid_rows,
    };
    let manifest = Manifest {
        format_version: MANIFEST_FORMAT_VERSION,
        backup_id: backup_id.to_string(),
        db_sha256: db_sha256.to_string(),
        images,
        missing,
        invalid_rows: referenced.invalid_rows,
    };
    let json = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| internal("failed to encode image manifest", e))?;
    write_file_atomic(&manifest_path_for(db_backup_path), &json)
        .map_err(|e| internal("failed to write image manifest", e))?;
    Ok(counts)
}

/// What a restore has to do about images.
#[derive(Debug)]
pub enum ImagePlan {
    /// Backup predates image backups: there is nothing to restore; report what is missing.
    Legacy { referenced: Vec<String> },
    /// Verified: these images are in the store, hash-checked, and match the snapshot.
    Verified {
        to_restore: Vec<ManifestImage>,
        missing_at_backup: usize,
    },
}

/// Validates the manifest and every stored image against the checksummed database backup.
/// Read-only: no live state is touched. Errors never contain paths.
pub fn plan_restore(
    db_backup_path: &Path,
    backup_id: &str,
    verified_db_sha256: &str,
) -> Result<ImagePlan, ApiError> {
    let referenced = {
        let conn = Connection::open_with_flags(db_backup_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| internal("failed to open backup for image check", e))?;
        referenced_images(&conn)?
    };

    let mpath = manifest_path_for(db_backup_path);
    match std::fs::symlink_metadata(&mpath) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ImagePlan::Legacy {
                referenced: referenced.images.keys().cloned().collect(),
            });
        }
        Err(_) => return Err(damaged()),
        Ok(meta) if !meta.is_file() || meta.len() > MAX_MANIFEST_BYTES => return Err(damaged()),
        Ok(_) => {}
    }

    let bytes = std::fs::read(&mpath).map_err(|_| damaged())?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|_| damaged())?;

    if manifest.format_version != MANIFEST_FORMAT_VERSION
        || manifest.backup_id != backup_id
        || !manifest.db_sha256.eq_ignore_ascii_case(verified_db_sha256)
        || manifest.images.len() > MAX_MANIFEST_IMAGES
        || manifest.missing.len() > MAX_MANIFEST_IMAGES
    {
        return Err(damaged());
    }

    // Structure: valid, unique names whose stem is the claimed hash; no conflicts.
    let mut seen = BTreeSet::new();
    for img in &manifest.images {
        let stem_matches = img.name.split_once('.').map(|(s, _)| s) == Some(img.sha256.as_str());
        if !is_valid_image_name(&img.name)
            || !stem_matches
            || img.size_bytes == 0
            || img.size_bytes > MAX_IMAGE_BYTES
            || !seen.insert(img.name.clone())
        {
            return Err(damaged());
        }
    }
    for name in &manifest.missing {
        if !is_valid_image_name(name) || !seen.insert(name.clone()) {
            return Err(damaged());
        }
    }

    // The manifest must account for exactly the images the snapshot refers to.
    let referenced_names: BTreeSet<String> = referenced.images.keys().cloned().collect();
    if seen != referenced_names {
        return Err(damaged());
    }

    // Every stored image must be intact.
    let store = store_dir_for(db_backup_path);
    for img in &manifest.images {
        match read_regular_file(&store.join(&img.name)) {
            Some(b) if b.len() as u64 == img.size_bytes && sha256_hex(&b) == img.sha256 => {}
            _ => return Err(damaged()),
        }
    }

    Ok(ImagePlan::Verified {
        missing_at_backup: manifest.missing.len() + manifest.invalid_rows,
        to_restore: manifest.images,
    })
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StageSummary {
    pub restored: usize,
    pub already_present: usize,
}

/// Restore side, step before the database is touched: makes every verified image available
/// in the live upload directory. Additive only: a live file that already has the right
/// content is left alone; a live file under the same name with the wrong content is moved
/// aside (never deleted) before the verified copy is put in place.
pub fn stage_images(
    db_backup_path: &Path,
    upload_dir: &Path,
    images: &[ManifestImage],
    restore_tag: &str,
) -> Result<StageSummary, ApiError> {
    std::fs::create_dir_all(upload_dir)
        .map_err(|e| internal("failed to create upload directory", e))?;
    let store = store_dir_for(db_backup_path);
    let mut summary = StageSummary::default();

    for img in images {
        // Re-verify at copy time; the store is not locked between the check and now.
        let bytes = match read_regular_file(&store.join(&img.name)) {
            Some(b) if b.len() as u64 == img.size_bytes && sha256_hex(&b) == img.sha256 => b,
            _ => return Err(damaged()),
        };
        let dst = upload_dir.join(&img.name);

        match std::fs::symlink_metadata(&dst) {
            Ok(meta) => {
                let intact = meta.is_file()
                    && meta.len() == img.size_bytes
                    && std::fs::read(&dst).map(|b| sha256_hex(&b)).ok().as_deref()
                        == Some(img.sha256.as_str());
                if intact {
                    summary.already_present += 1;
                    continue;
                }
                let aside = upload_dir.join(format!("{}.corrupt-{restore_tag}", img.name));
                std::fs::rename(&dst, &aside)
                    .map_err(|e| internal("failed to set aside a damaged bill image", e))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(internal("failed to inspect upload directory", e)),
        }

        write_file_atomic(&dst, &bytes)
            .map_err(|e| internal("failed to restore a bill image", e))?;
        summary.restored += 1;
    }
    Ok(summary)
}

/// How many of `names` have no live file in the upload directory.
pub fn count_missing_live(upload_dir: &Path, names: &[String]) -> usize {
    names
        .iter()
        .filter(|n| regular_file_len(&upload_dir.join(n)).is_none())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_content_addressed_names_are_valid() {
        let sha = "a".repeat(64);
        assert!(is_valid_image_name(&format!("{sha}.png")));
        assert!(is_valid_image_name(&format!("{sha}.jpg")));
        assert!(is_valid_image_name(&format!("{sha}.webp")));
        for bad in [
            "".to_string(),
            "x.png".to_string(),
            format!("{sha}.gif"),
            format!("{sha}.PNG"),
            format!("{sha}.png.exe"),
            format!("{}.png", "A".repeat(64)),
            format!("../{sha}.png"),
            format!("{sha}"),
            format!("{}.png", "a".repeat(63)),
        ] {
            assert!(!is_valid_image_name(&bad), "{bad:?}");
        }
    }

    #[test]
    fn stored_paths_resolve_by_file_name_across_machines_and_reject_traversal() {
        let sha = "b".repeat(64);
        let up = Path::new("/srv/uploads");
        let name = format!("{sha}.png");
        // Absolute paths from another machine / OS resolve into the *current* directory.
        for stored in [
            format!("/old/machine/data/uploads/{name}"),
            format!("C:\\Users\\shop\\data\\uploads\\{name}"),
            name.clone(),
        ] {
            assert_eq!(resolve_upload_path(up, &stored), Some(up.join(&name)), "{stored}");
        }
        for stored in [
            "../../etc/passwd".to_string(),
            "/etc/passwd".to_string(),
            format!("{sha}.png/../../x"),
            format!("{sha}.png/"),
            String::new(),
        ] {
            assert_eq!(resolve_upload_path(up, &stored), None, "{stored:?}");
        }
    }

    #[test]
    fn manifest_path_sits_next_to_the_database_backup() {
        let p = manifest_path_for(Path::new("/b/artha-backup-123.sqlite3"));
        assert_eq!(p, PathBuf::from("/b/artha-backup-123.images.json"));
        assert_eq!(
            store_dir_for(Path::new("/b/artha-backup-123.sqlite3")),
            PathBuf::from("/b/images")
        );
    }
}
