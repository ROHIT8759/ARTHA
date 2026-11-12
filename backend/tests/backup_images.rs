//! Backup/restore of purchase bill photos (Phase 10 hardening of Phase 09 backups).
//!
//! A backup is the SQLite snapshot plus a copy of every photo that snapshot refers to,
//! kept in a shared content-addressed store next to the backups with a manifest bound to
//! the snapshot's checksum. Restore verifies everything before touching live state, copies
//! photos in additively, and only then restores the database.

use artha_server::{build_app, db, AppState};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use tower::ServiceExt;

fn unique_dir(label: &str, kind: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "artha_bimg_{label}_{kind}_{}_{n}",
        std::process::id()
    ))
}

struct Env {
    app: axum::Router,
    state: AppState,
    token: String,
    backup_dir: PathBuf,
    upload_dir: PathBuf,
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

fn json_request(method: &str, uri: &str, token: Option<&str>, body: Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

fn multipart_request(uri: &str, token: &str, bytes: &[u8]) -> Request<Body> {
    let boundary = "----artha-test-boundary";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"bill.png\"\r\nContent-Type: image/png\r\n\r\n",
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap()
}

/// A tiny file with genuine PNG magic bytes; `seed` makes each one distinct.
fn png(seed: u8) -> Vec<u8> {
    let mut v = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
    ];
    v.extend((0..200u32).map(|i| (i as u8).wrapping_add(seed)));
    v
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

async fn env_with(label: &str, retention: usize) -> Env {
    let mut state = AppState::for_tests(db::open_in_memory());
    state.backup_dir = unique_dir(label, "backups");
    state.upload_dir = unique_dir(label, "uploads");
    state.backup_retention = retention;
    let (backup_dir, upload_dir) = (state.backup_dir.clone(), state.upload_dir.clone());
    let app = build_app(state.clone());
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/setup",
            None,
            json!({ "business_name": "Shop", "owner_username": "admin", "owner_password": "password123" }),
        ))
        .await
        .unwrap();
    let token = body_json(resp).await["token"].as_str().unwrap().to_string();
    Env {
        app,
        state,
        token,
        backup_dir,
        upload_dir,
    }
}

async fn env(label: &str) -> Env {
    env_with(label, 24).await
}

impl Env {
    async fn send(&self, req: Request<Body>) -> (StatusCode, Value) {
        let resp = self.app.clone().oneshot(req).await.unwrap();
        (resp.status(), body_json(resp).await)
    }

    /// Creates a draft purchase and attaches `bytes` as its bill photo.
    /// Returns (purchase_id, image_id).
    async fn purchase_with_photo(&self, bytes: &[u8]) -> (String, String) {
        let (st, p) = self
            .send(json_request(
                "POST",
                "/api/purchases",
                Some(&self.token),
                json!({ "source": "ocr" }),
            ))
            .await;
        assert_eq!(st, StatusCode::OK);
        let pid = p["purchase_id"].as_str().unwrap().to_string();
        let (st, img) = self
            .send(multipart_request(
                &format!("/api/purchases/{pid}/image"),
                &self.token,
                bytes,
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "{img}");
        (pid, img["image_id"].as_str().unwrap().to_string())
    }

    async fn backup(&self) -> Value {
        let (st, v) = self
            .send(json_request(
                "POST",
                "/api/backups",
                Some(&self.token),
                Value::Null,
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "backup should succeed: {v}");
        v
    }

    async fn restore(&self, backup_id: &str) -> (StatusCode, Value) {
        self.send(json_request(
            "POST",
            &format!("/api/backups/{backup_id}/restore"),
            Some(&self.token),
            json!({ "confirm": true }),
        ))
        .await
    }

    async fn verify(&self, backup_id: &str) -> Value {
        let (st, v) = self
            .send(json_request(
                "POST",
                &format!("/api/backups/{backup_id}/verify"),
                Some(&self.token),
                Value::Null,
            ))
            .await;
        assert_eq!(st, StatusCode::OK);
        v
    }

    async fn list(&self) -> Vec<Value> {
        let (st, v) = self
            .send(json_request(
                "GET",
                "/api/backups",
                Some(&self.token),
                Value::Null,
            ))
            .await;
        assert_eq!(st, StatusCode::OK);
        v.as_array().unwrap().clone()
    }

    async fn get_photo(&self, app: &axum::Router, purchase_id: &str) -> (StatusCode, Vec<u8>) {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/purchases/{purchase_id}/image"))
                    .header("authorization", format!("Bearer {}", self.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let st = resp.status();
        (
            st,
            resp.into_body().collect().await.unwrap().to_bytes().to_vec(),
        )
    }

    async fn product_count(&self) -> usize {
        let (st, v) = self
            .send(json_request(
                "GET",
                "/api/products",
                Some(&self.token),
                Value::Null,
            ))
            .await;
        assert_eq!(st, StatusCode::OK);
        v.as_array().map(|a| a.len()).unwrap_or(0)
    }

    async fn add_product(&self, name: &str) {
        let (st, v) = self
            .send(json_request(
                "POST",
                "/api/products",
                Some(&self.token),
                json!({ "name": name, "qr_code": format!("QR-{name}"), "price_paise": 5000, "cost_paise": 4000, "gst_rate_bps": 500 }),
            ))
            .await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
}

fn manifest_path(backup: &Value) -> PathBuf {
    Path::new(backup["path"].as_str().unwrap()).with_extension("images.json")
}

fn store_file(env: &Env, photo: &[u8]) -> PathBuf {
    env.backup_dir.join("images").join(format!("{}.png", sha(photo)))
}

fn live_file(env: &Env, photo: &[u8]) -> PathBuf {
    env.upload_dir.join(format!("{}.png", sha(photo)))
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

#[tokio::test]
async fn a_backup_contains_the_bill_photos_its_snapshot_refers_to() {
    let e = env("contains").await;
    let photo = png(1);
    e.purchase_with_photo(&photo).await;

    let b = e.backup().await;
    assert_eq!(b["images_included"], true);
    assert_eq!(b["image_count"], 1);
    assert_eq!(b["images_missing"], 0);
    assert_eq!(std::fs::read(store_file(&e, &photo)).unwrap(), photo);
    assert!(manifest_path(&b).is_file());

    let listed = &e.list().await[0];
    assert_eq!(listed["images_included"], true);
    assert_eq!(listed["image_count"], 1);

    let v = e.verify(b["backup_id"].as_str().unwrap()).await;
    assert_eq!(v["images_included"], true);
    assert_eq!(v["images_ok"], true);
    assert_eq!(v["valid"], true);
    assert_eq!(v["images"]["checked"], 1);
}

#[tokio::test]
async fn restore_recovers_a_lost_photo_and_keeps_the_purchase_association() {
    let e = env("recover").await;
    let photo = png(2);
    let (pid, image_id) = e.purchase_with_photo(&photo).await;
    let b = e.backup().await;
    let id = b["backup_id"].as_str().unwrap();

    // Disaster: the upload folder is lost, and the database moves on.
    std::fs::remove_file(live_file(&e, &photo)).unwrap();
    e.add_product("AfterBackup").await;
    assert_eq!(e.get_photo(&e.app, &pid).await.0, StatusCode::NOT_FOUND);

    let (st, r) = e.restore(id).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["images"]["included"], true);
    assert_eq!(r["images"]["restored"], 1);
    assert_eq!(r["images"]["missing"], 0);

    // The photo is back, byte for byte, and still belongs to the same purchase.
    let (st, bytes) = e.get_photo(&e.app, &pid).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(bytes, photo);
    let (_, view) = e
        .send(json_request(
            "GET",
            &format!("/api/purchases/{pid}"),
            Some(&e.token),
            Value::Null,
        ))
        .await;
    assert_eq!(view["image_id"], image_id.as_str());
    // ... and the database really was rolled back.
    assert_eq!(e.product_count().await, 0);
}

#[tokio::test]
async fn restore_works_after_the_upload_directory_moved() {
    let e = env("moved").await;
    let photo = png(3);
    let (pid, _) = e.purchase_with_photo(&photo).await;
    let b = e.backup().await;

    // Same database, new upload location (for example restoring on a new PC): the stored
    // absolute path is stale, so photos must be located by name in the configured folder.
    let mut moved = e.state.clone();
    moved.upload_dir = unique_dir("moved2", "uploads");
    let app2 = build_app(moved.clone());

    let (st, r) = {
        let resp = app2
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/backups/{}/restore", b["backup_id"].as_str().unwrap()),
                Some(&e.token),
                json!({ "confirm": true }),
            ))
            .await
            .unwrap();
        (resp.status(), body_json(resp).await)
    };
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["images"]["restored"], 1);
    let (st, bytes) = e.get_photo(&app2, &pid).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(bytes, photo);
    assert!(moved
        .upload_dir
        .join(format!("{}.png", sha(&photo)))
        .is_file());
}

#[tokio::test]
async fn a_tampered_stored_photo_blocks_restore_and_changes_nothing() {
    let e = env("tampered").await;
    let photo = png(4);
    let (pid, _) = e.purchase_with_photo(&photo).await;
    let b = e.backup().await;
    let id = b["backup_id"].as_str().unwrap();

    // Flip a byte in the backup copy (same length, so only the hash can catch it).
    let mut bad = photo.clone();
    let last = bad.len() - 1;
    bad[last] ^= 0xFF;
    std::fs::write(store_file(&e, &photo), &bad).unwrap();

    // Verification reports it ...
    let v = e.verify(id).await;
    assert_eq!(v["images_ok"], false);
    assert_eq!(v["valid"], false);

    // ... and restore refuses, with live data untouched.
    std::fs::remove_file(live_file(&e, &photo)).unwrap();
    e.add_product("LiveData").await;
    let (st, r) = e.restore(id).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{r}");
    let msg = r["message"].as_str().unwrap();
    assert!(msg.contains("refusing to restore"), "{msg}");
    assert!(!msg.contains(&*e.backup_dir.to_string_lossy()), "path leaked: {msg}");
    assert_eq!(e.product_count().await, 1, "database must not have been rolled back");
    assert!(!live_file(&e, &photo).exists(), "nothing may be written to the live folder");
    assert_eq!(e.get_photo(&e.app, &pid).await.0, StatusCode::NOT_FOUND);
    assert_eq!(e.list().await.len(), 1, "no safety backup for a refused restore");
}

#[tokio::test]
async fn a_missing_stored_photo_blocks_restore() {
    let e = env("missing_store").await;
    let photo = png(5);
    e.purchase_with_photo(&photo).await;
    let b = e.backup().await;
    std::fs::remove_file(store_file(&e, &photo)).unwrap();

    let (st, _) = e.restore(b["backup_id"].as_str().unwrap()).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert_eq!(e.verify(b["backup_id"].as_str().unwrap()).await["valid"], false);
}

#[tokio::test]
async fn malformed_or_hostile_manifests_block_restore() {
    let e = env("manifest").await;
    let photo = png(6);
    e.purchase_with_photo(&photo).await;
    let b = e.backup().await;
    let id = b["backup_id"].as_str().unwrap().to_string();
    let mpath = manifest_path(&b);
    let original: Value = serde_json::from_slice(&std::fs::read(&mpath).unwrap()).unwrap();
    let name = format!("{}.png", sha(&photo));
    let evil = e.backup_dir.parent().unwrap().join("artha_evil.png");
    let _ = std::fs::remove_file(&evil);

    let cases: Vec<(&str, Box<dyn Fn(&mut Value)>)> = vec![
        ("wrong backup id", Box::new(|m: &mut Value| m["backup_id"] = json!("someone-else"))),
        ("wrong db checksum", Box::new(|m: &mut Value| m["db_sha256"] = json!("0".repeat(64)))),
        ("unknown format version", Box::new(|m: &mut Value| m["format_version"] = json!(99))),
        ("unknown field", Box::new(|m: &mut Value| m["extra"] = json!(true))),
        ("empty image list", Box::new(|m: &mut Value| m["images"] = json!([]))),
        (
            "path traversal name",
            Box::new(|m: &mut Value| {
                m["images"][0]["name"] = json!("../artha_evil.png");
            }),
        ),
        (
            "absolute path name",
            Box::new(|m: &mut Value| {
                m["images"][0]["name"] = json!("/etc/passwd");
            }),
        ),
        (
            "name does not match its hash",
            Box::new(|m: &mut Value| {
                m["images"][0]["sha256"] = json!("1".repeat(64));
            }),
        ),
        (
            "duplicate entries",
            Box::new(|m: &mut Value| {
                let first = m["images"][0].clone();
                m["images"].as_array_mut().unwrap().push(first);
            }),
        ),
        (
            "extra image the database does not refer to",
            Box::new(|m: &mut Value| {
                let other = "2".repeat(64);
                m["images"].as_array_mut().unwrap().push(
                    json!({ "name": format!("{other}.png"), "sha256": other, "size_bytes": 10 }),
                );
            }),
        ),
        (
            "wrong size",
            Box::new(|m: &mut Value| m["images"][0]["size_bytes"] = json!(1)),
        ),
        (
            "image listed as both stored and missing",
            Box::new({
                let name = name.clone();
                move |m: &mut Value| m["missing"] = json!([name.clone()])
            }),
        ),
    ];

    for (label, mutate) in cases {
        let mut m = original.clone();
        mutate(&mut m);
        std::fs::write(&mpath, serde_json::to_vec(&m).unwrap()).unwrap();
        let (st, r) = e.restore(&id).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{label}: {r}");
        assert_eq!(e.verify(&id).await["valid"], false, "{label}");
    }
    assert!(!evil.exists(), "a hostile manifest must never create files outside the folders");

    // Not even JSON.
    std::fs::write(&mpath, b"{ this is not json").unwrap();
    assert_eq!(e.restore(&id).await.0, StatusCode::BAD_REQUEST);

    // The untouched manifest still restores fine.
    std::fs::write(&mpath, serde_json::to_vec(&original).unwrap()).unwrap();
    assert_eq!(e.restore(&id).await.0, StatusCode::OK);
}

#[cfg(unix)]
#[tokio::test]
async fn a_symlinked_stored_photo_is_rejected() {
    let e = env("symlink").await;
    let photo = png(7);
    e.purchase_with_photo(&photo).await;
    let b = e.backup().await;

    // Replace the stored copy with a symlink to a file holding the right bytes.
    let target = unique_dir("symlink", "target");
    std::fs::write(&target, &photo).unwrap();
    let store = store_file(&e, &photo);
    std::fs::remove_file(&store).unwrap();
    std::os::unix::fs::symlink(&target, &store).unwrap();

    assert_eq!(
        e.restore(b["backup_id"].as_str().unwrap()).await.0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn a_legacy_database_only_backup_still_restores_and_reports_missing_photos() {
    let e = env("legacy").await;
    let photo = png(8);
    let (pid, _) = e.purchase_with_photo(&photo).await;
    let b = e.backup().await;
    let id = b["backup_id"].as_str().unwrap();

    // Turn it into a backup made before photos were included.
    std::fs::remove_file(manifest_path(&b)).unwrap();
    std::fs::remove_dir_all(e.backup_dir.join("images")).unwrap();
    let listed = &e.list().await[0];
    assert_eq!(listed["images_included"], false);
    let v = e.verify(id).await;
    assert_eq!(v["images_included"], false);
    assert_eq!(v["valid"], true, "a legacy backup is still a valid backup");

    // Photo still on disk: restore succeeds and says nothing is missing.
    let (st, r) = e.restore(id).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["images"]["included"], false);
    assert_eq!(r["images"]["missing"], 0);
    assert_eq!(r["images"]["already_present"], 1);

    // Photo gone from disk: restore still succeeds (as before), but the gap is reported.
    std::fs::remove_file(live_file(&e, &photo)).unwrap();
    let (st, r) = e.restore(id).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["images"]["missing"], 1);
    assert_eq!(e.get_photo(&e.app, &pid).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_failed_backup_leaves_no_partial_files_and_no_row() {
    let e = env("failed_backup").await;
    e.purchase_with_photo(&png(9)).await;
    // Sabotage: a regular file sits where the photo store directory must be created.
    std::fs::create_dir_all(&e.backup_dir).unwrap();
    std::fs::write(e.backup_dir.join("images"), b"in the way").unwrap();

    let (st, _) = e
        .send(json_request(
            "POST",
            "/api/backups",
            Some(&e.token),
            Value::Null,
        ))
        .await;
    assert_eq!(st, StatusCode::INTERNAL_SERVER_ERROR);

    assert!(e.list().await.is_empty(), "no backup row may be registered");
    let leftovers: Vec<String> = names_in(&e.backup_dir)
        .into_iter()
        .filter(|n| n != "images")
        .collect();
    assert!(leftovers.is_empty(), "partial backup files left behind: {leftovers:?}");
}

#[tokio::test]
async fn a_failed_photo_restore_leaves_the_database_and_safety_backup_intact() {
    let e = env("failed_restore").await;
    let photo = png(10);
    e.purchase_with_photo(&photo).await;
    let b = e.backup().await;
    e.add_product("LiveData").await;

    // Sabotage: the upload folder cannot be created because a file occupies its name.
    std::fs::remove_dir_all(&e.upload_dir).unwrap();
    std::fs::write(&e.upload_dir, b"in the way").unwrap();

    let (st, r) = e.restore(b["backup_id"].as_str().unwrap()).await;
    assert_eq!(st, StatusCode::INTERNAL_SERVER_ERROR, "{r}");
    assert_eq!(r["error"], "restore_failed");
    let safety = r["safety_backup_id"].as_str().unwrap();
    assert!(e.list().await.iter().any(|v| v["backup_id"] == safety));
    assert!(r["message"].as_str().unwrap().contains("not changed"));

    // The database was never replaced: the post-backup product is still there.
    assert_eq!(e.product_count().await, 1);
}

#[tokio::test]
async fn retention_removes_old_manifests_but_never_the_shared_photos() {
    let e = env_with("retention", 1).await;
    let photo = png(11);
    e.purchase_with_photo(&photo).await;
    let mut ids = Vec::new();
    for _ in 0..3 {
        ids.push(e.backup().await["backup_id"].as_str().unwrap().to_string());
        // Backup ids order by creation time at millisecond granularity.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }

    assert_eq!(e.list().await.len(), 1, "retention keeps one backup");
    let manifests: Vec<String> = names_in(&e.backup_dir)
        .into_iter()
        .filter(|n| n.ends_with(".images.json"))
        .collect();
    assert_eq!(manifests.len(), 1, "pruned backups take their manifests with them");
    assert_eq!(std::fs::read(store_file(&e, &photo)).unwrap(), photo, "shared photos are never pruned");
    let _ = ids;
}

#[tokio::test]
async fn the_safety_backup_taken_by_a_restore_includes_photos() {
    let e = env("safety").await;
    let photo = png(12);
    e.purchase_with_photo(&photo).await;
    let b = e.backup().await;
    // A second photo exists only in the live state at restore time.
    let live_only = png(13);
    e.purchase_with_photo(&live_only).await;

    let (st, r) = e.restore(b["backup_id"].as_str().unwrap()).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let safety_id = r["safety_backup_id"].as_str().unwrap();
    let safety = e
        .list()
        .await
        .into_iter()
        .find(|v| v["backup_id"] == safety_id)
        .unwrap();
    assert_eq!(safety["images_included"], true);
    assert_eq!(safety["image_count"], 2);
    // The photo that is not part of the restored snapshot is kept, not deleted.
    assert!(live_file(&e, &live_only).is_file());
    assert!(store_file(&e, &live_only).is_file());
}

#[tokio::test]
async fn a_damaged_live_photo_is_set_aside_not_deleted_when_restoring() {
    let e = env("aside").await;
    let photo = png(14);
    e.purchase_with_photo(&photo).await;
    let b = e.backup().await;

    let mut damaged = photo.clone();
    damaged[20] ^= 0xFF;
    std::fs::write(live_file(&e, &photo), &damaged).unwrap();

    let (st, r) = e.restore(b["backup_id"].as_str().unwrap()).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["images"]["restored"], 1);
    assert_eq!(std::fs::read(live_file(&e, &photo)).unwrap(), photo);
    let aside: Vec<String> = names_in(&e.upload_dir)
        .into_iter()
        .filter(|n| n.contains(".corrupt-"))
        .collect();
    assert_eq!(aside.len(), 1, "{aside:?}");
    assert_eq!(std::fs::read(e.upload_dir.join(&aside[0])).unwrap(), damaged);
}

#[tokio::test]
async fn database_paths_cannot_make_the_server_read_or_back_up_files_outside_the_upload_folder() {
    let e = env("hostile_path").await;
    let (pid, _) = e.purchase_with_photo(&png(15)).await;

    // A secret outside the upload folder with a valid-looking image name.
    let secret = b"top secret".to_vec();
    let parent = e.upload_dir.parent().unwrap().to_path_buf();
    let secret_path = parent.join(format!("{}.png", sha(&secret)));
    std::fs::write(&secret_path, &secret).unwrap();

    // Point the purchase's photo row (and an extra row) at it, as a hostile restored
    // database could.
    {
        let conn = e.state.db.lock().unwrap();
        conn.execute(
            "UPDATE bill_images SET path = ?1 WHERE purchase_id = ?2",
            rusqlite::params![secret_path.to_string_lossy(), pid],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO bill_images (image_id, purchase_id, path, sha256_hex, created_at) VALUES ('x', ?1, '../../etc/passwd', 'abc', '2026-01-01T00:00:00.000Z')",
            rusqlite::params![pid],
        )
        .unwrap();
    }

    // Download resolves by name inside the upload folder only: the secret is not served.
    let (st, bytes) = e.get_photo(&e.app, &pid).await;
    assert_ne!(st, StatusCode::OK);
    assert_ne!(bytes, secret);

    // Backing up does not copy it either, and the gap is reported, not hidden.
    let b = e.backup().await;
    assert_eq!(b["image_count"], 0);
    assert!(b["images_missing"].as_u64().unwrap() >= 1);
    assert!(!e.backup_dir.join("images").join(secret_path.file_name().unwrap()).exists());
    let _ = std::fs::remove_file(secret_path);
}
