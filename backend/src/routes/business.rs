//! Business Profile & Document Configuration (Phase 08).
//!
//! Handles shop identity, contact details, tax info (GSTIN), document layout
//! preferences (A4 vs thermal), print headers/footers, and safe raster logo storage.
//! Enforces Owner-only mutations while allowing authenticated Staff to read
//! business details for billing and invoice generation.

use axum::extract::State;
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::ids::now_iso;
use crate::models::{BusinessSettingsView, BusinessView, UpdateBusinessRequest, UploadLogoRequest};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/business", get(get_business).patch(update_business))
        .route("/api/business/logo", post(upload_logo).delete(remove_logo))
}

fn fetch_business(conn: &rusqlite::Connection, business_id: &str) -> Result<BusinessView, ApiError> {
    let row = conn
        .query_row(
            "SELECT business_id, name, settings_json, created_at, updated_at \
             FROM businesses WHERE business_id = ?1",
            rusqlite::params![business_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            },
        )
        .map_err(|_| ApiError::NotFound("Business not found".into()))?;

    let (biz_id, name, settings_json, created_at, updated_at) = row;
    let settings: BusinessSettingsView =
        serde_json::from_str(&settings_json).unwrap_or_default();

    Ok(BusinessView {
        business_id: biz_id,
        name,
        settings,
        created_at,
        updated_at,
    })
}

/// GET /api/business - Retrieve business profile and invoice settings (Staff & Owner)
async fn get_business(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<BusinessView>, ApiError> {
    let conn = state.db.lock().expect("db mutex poisoned");
    let biz = fetch_business(&conn, &user.business_id)?;
    Ok(Json(biz))
}

/// PATCH /api/business - Update business identity and document settings (Owner only)
async fn update_business(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<UpdateBusinessRequest>,
) -> Result<Json<BusinessView>, ApiError> {
    // 1. Authorization: Only Owner can modify business profile and document configuration
    user.require_owner()?;

    // 2. Validate input fields
    if let Some(ref name) = req.name {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(ApiError::BadRequest("Business name cannot be empty".into()));
        }
        if trimmed.len() > 100 {
            return Err(ApiError::BadRequest("Business name cannot exceed 100 characters".into()));
        }
    }

    if let Some(ref email) = req.email {
        let trimmed = email.trim();
        if !trimmed.is_empty() && (!trimmed.contains('@') || !trimmed.contains('.')) {
            return Err(ApiError::BadRequest("Invalid email address format".into()));
        }
        if trimmed.len() > 100 {
            return Err(ApiError::BadRequest("Email cannot exceed 100 characters".into()));
        }
    }

    if let Some(ref phone) = req.phone {
        let trimmed = phone.trim();
        if trimmed.len() > 30 {
            return Err(ApiError::BadRequest("Phone number cannot exceed 30 characters".into()));
        }
    }

    if let Some(ref gstin) = req.gstin {
        let trimmed = gstin.trim();
        if trimmed.len() > 20 {
            return Err(ApiError::BadRequest("GSTIN cannot exceed 20 characters".into()));
        }
    }

    if let Some(ref layout) = req.default_layout {
        let l = layout.trim().to_lowercase();
        if l != "a4" && l != "thermal" {
            return Err(ApiError::BadRequest("Default layout must be 'a4' or 'thermal'".into()));
        }
    }

    let mut conn = state.db.lock().expect("db mutex poisoned");

    // Fetch existing settings
    let current = fetch_business(&conn, &user.business_id)?;
    let mut settings = current.settings;

    // Apply updates
    let updated_name = req.name.map(|n| n.trim().to_string()).unwrap_or(current.name);

    if let Some(rn) = req.registered_name {
        settings.registered_name = if rn.trim().is_empty() { None } else { Some(rn.trim().to_string()) };
    }
    if let Some(addr) = req.address {
        settings.address = if addr.trim().is_empty() { None } else { Some(addr.trim().to_string()) };
    }
    if let Some(c) = req.city {
        settings.city = if c.trim().is_empty() { None } else { Some(c.trim().to_string()) };
    }
    if let Some(s) = req.state {
        settings.state = if s.trim().is_empty() { None } else { Some(s.trim().to_string()) };
    }
    if let Some(p) = req.pincode {
        settings.pincode = if p.trim().is_empty() { None } else { Some(p.trim().to_string()) };
    }
    if let Some(co) = req.country {
        settings.country = if co.trim().is_empty() { None } else { Some(co.trim().to_string()) };
    }
    if let Some(ph) = req.phone {
        settings.phone = if ph.trim().is_empty() { None } else { Some(ph.trim().to_string()) };
    }
    if let Some(em) = req.email {
        settings.email = if em.trim().is_empty() { None } else { Some(em.trim().to_string()) };
    }
    if let Some(g) = req.gstin {
        settings.gstin = if g.trim().is_empty() { None } else { Some(g.trim().to_uppercase()) };
    }
    if let Some(footer) = req.invoice_footer {
        settings.invoice_footer = if footer.trim().is_empty() { None } else { Some(footer.trim().to_string()) };
    }
    if let Some(terms) = req.terms_and_conditions {
        settings.terms_and_conditions = if terms.trim().is_empty() { None } else { Some(terms.trim().to_string()) };
    }
    if let Some(layout) = req.default_layout {
        settings.default_layout = Some(layout.trim().to_lowercase());
    }
    if let Some(sh) = req.show_hsn {
        settings.show_hsn = Some(sh);
    }
    if let Some(st) = req.show_tax_breakdown {
        settings.show_tax_breakdown = Some(st);
    }
    if let Some(sl) = req.show_logo {
        settings.show_logo = Some(sl);
    }

    let settings_json = serde_json::to_string(&settings)
        .map_err(|e| ApiError::Internal(format!("Failed to serialize settings: {e}")))?;

    let now = now_iso();
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE businesses SET name = ?1, settings_json = ?2, updated_at = ?3 WHERE business_id = ?4",
        rusqlite::params![updated_name, settings_json, now, user.business_id],
    )?;

    crate::audit::record(
        &tx,
        &user.business_id,
        Some(&user.user_id),
        "update_business_settings",
        "business",
        Some(&user.business_id),
        &serde_json::json!({ "name": updated_name }).to_string(),
    )?;
    tx.commit()?;

    fetch_business(&conn, &user.business_id).map(Json)
}

/// Maximum permitted binary decoded logo size: 1 MiB (1,048,576 bytes)
const MAX_LOGO_BYTES: usize = 1_048_576;

/// Strictly decodes standard RFC 4648 Base64 without external crates.
/// Rejects non-base64 characters, improper length, invalid padding, and dirty padding bits.
fn strict_decode_base64(input: &str) -> Result<Vec<u8>, ApiError> {
    if input.is_empty() {
        return Err(ApiError::BadRequest("Base64 payload is empty".into()));
    }

    let bytes = input.as_bytes();
    if bytes.len() % 4 != 0 {
        return Err(ApiError::BadRequest("Base64 payload length must be a multiple of 4".into()));
    }

    let mut out = Vec::with_capacity((bytes.len() / 4) * 3);

    let decode_char = |b: u8| -> Result<u8, ApiError> {
        match b {
            b'A'..=b'Z' => Ok(b - b'A'),
            b'a'..=b'z' => Ok(b - b'a' + 26),
            b'0'..=b'9' => Ok(b - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(ApiError::BadRequest("Invalid Base64 character in image payload".into())),
        }
    };

    let chunks = bytes.chunks_exact(4);
    let chunk_count = chunks.len();

    for (chunk_idx, chunk) in chunks.enumerate() {
        let is_last = chunk_idx == chunk_count - 1;

        if is_last {
            if chunk[2] == b'=' {
                if chunk[3] != b'=' {
                    return Err(ApiError::BadRequest("Invalid Base64 padding structure".into()));
                }
                let c0 = decode_char(chunk[0])?;
                let c1 = decode_char(chunk[1])?;
                if c1 & 0x0F != 0 {
                    return Err(ApiError::BadRequest("Invalid Base64 padding bits".into()));
                }
                out.push((c0 << 2) | (c1 >> 4));
            } else if chunk[3] == b'=' {
                let c0 = decode_char(chunk[0])?;
                let c1 = decode_char(chunk[1])?;
                let c2 = decode_char(chunk[2])?;
                if c2 & 0x03 != 0 {
                    return Err(ApiError::BadRequest("Invalid Base64 padding bits".into()));
                }
                out.push((c0 << 2) | (c1 >> 4));
                out.push((c1 << 4) | (c2 >> 2));
            } else {
                let c0 = decode_char(chunk[0])?;
                let c1 = decode_char(chunk[1])?;
                let c2 = decode_char(chunk[2])?;
                let c3 = decode_char(chunk[3])?;
                out.push((c0 << 2) | (c1 >> 4));
                out.push((c1 << 4) | (c2 >> 2));
                out.push((c2 << 6) | c3);
            }
        } else {
            if chunk.contains(&b'=') {
                return Err(ApiError::BadRequest("Base64 padding '=' found before end of payload".into()));
            }
            let c0 = decode_char(chunk[0])?;
            let c1 = decode_char(chunk[1])?;
            let c2 = decode_char(chunk[2])?;
            let c3 = decode_char(chunk[3])?;
            out.push((c0 << 2) | (c1 >> 4));
            out.push((c1 << 4) | (c2 >> 2));
            out.push((c2 << 6) | c3);
        }

        if out.len() > MAX_LOGO_BYTES {
            return Err(ApiError::BadRequest("Decoded logo exceeds 1 MiB size limit".into()));
        }
    }

    Ok(out)
}

/// Strictly validates the logo data URL, decoding the binary bytes and verifying
/// allowed raster MIME types (PNG, JPEG, WebP) and magic byte signatures.
fn validate_logo_payload(data_url: &str) -> Result<(String, usize), ApiError> {
    if !data_url.starts_with("data:") {
        return Err(ApiError::BadRequest("Logo must be a data: URL".into()));
    }

    // Explicitly reject SVG and HTML
    let lower = data_url.to_lowercase();
    if lower.contains("image/svg") || lower.contains("<svg") || lower.contains("text/html") {
        return Err(ApiError::BadRequest(
            "SVG and HTML logos are strictly prohibited for security reasons. Please use PNG, JPEG, or WebP.".into(),
        ));
    }

    let parts: Vec<&str> = data_url.splitn(2, ',').collect();
    if parts.len() != 2 {
        return Err(ApiError::BadRequest("Missing data URL payload separator ','".into()));
    }

    let header = parts[0];
    let payload = parts[1].trim();

    if !header.ends_with(";base64") {
        return Err(ApiError::BadRequest("Logo data URL must declare ';base64' encoding".into()));
    }

    let mime = header.trim_start_matches("data:").trim_end_matches(";base64").to_lowercase();

    let normalized_mime = match mime.as_str() {
        "image/png" => "image/png",
        "image/jpeg" | "image/jpg" => "image/jpeg",
        "image/webp" => "image/webp",
        _ => {
            return Err(ApiError::BadRequest(
                "Unsupported image MIME type. Only PNG, JPEG, and WebP raster images are allowed".into(),
            ))
        }
    };

    let decoded = strict_decode_base64(payload)?;
    if decoded.is_empty() {
        return Err(ApiError::BadRequest("Decoded logo payload is empty".into()));
    }

    if decoded.len() > MAX_LOGO_BYTES {
        return Err(ApiError::BadRequest("Logo binary size exceeds 1 MiB limit".into()));
    }

    // Magic byte signature validation
    match normalized_mime {
        "image/png" => {
            const PNG_SIG: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
            if decoded.len() < 8 || &decoded[0..8] != &PNG_SIG {
                return Err(ApiError::BadRequest("File signature does not match declared PNG image format".into()));
            }
        }
        "image/jpeg" => {
            if decoded.len() < 3 || &decoded[0..3] != &[0xFF, 0xD8, 0xFF] {
                return Err(ApiError::BadRequest("File signature does not match declared JPEG image format".into()));
            }
        }
        "image/webp" => {
            if decoded.len() < 12 || &decoded[0..4] != b"RIFF" || &decoded[8..12] != b"WEBP" {
                return Err(ApiError::BadRequest("File signature does not match declared WebP image format".into()));
            }
        }
        _ => unreachable!(),
    }

    Ok((normalized_mime.to_string(), decoded.len()))
}

/// POST /api/business/logo - Upload business logo (strictly verified raster only, max 1MB)
async fn upload_logo(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<UploadLogoRequest>,
) -> Result<Json<BusinessView>, ApiError> {
    user.require_owner()?;

    let data_url = req.logo_data_url.trim();

    // 1. Strictly validate format, base64 payload, size, and image magic bytes
    let (mime, byte_size) = validate_logo_payload(data_url)?;

    let mut conn = state.db.lock().expect("db mutex poisoned");
    let current = fetch_business(&conn, &user.business_id)?;
    let mut settings = current.settings;
    settings.logo_data_url = Some(data_url.to_string());

    let settings_json = serde_json::to_string(&settings)
        .map_err(|e| ApiError::Internal(format!("Failed to serialize settings: {e}")))?;

    let now = now_iso();
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE businesses SET settings_json = ?1, updated_at = ?2 WHERE business_id = ?3",
        rusqlite::params![settings_json, now, user.business_id],
    )?;

    crate::audit::record(
        &tx,
        &user.business_id,
        Some(&user.user_id),
        "update_business_logo",
        "business",
        Some(&user.business_id),
        &serde_json::json!({
            "mime": mime,
            "byte_size": byte_size,
        })
        .to_string(),
    )?;
    tx.commit()?;

    fetch_business(&conn, &user.business_id).map(Json)
}

/// DELETE /api/business/logo - Remove business logo (Owner only)
async fn remove_logo(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<BusinessView>, ApiError> {
    user.require_owner()?;

    let mut conn = state.db.lock().expect("db mutex poisoned");
    let current = fetch_business(&conn, &user.business_id)?;
    let mut settings = current.settings;
    settings.logo_data_url = None;

    let settings_json = serde_json::to_string(&settings)
        .map_err(|e| ApiError::Internal(format!("Failed to serialize settings: {e}")))?;

    let now = now_iso();
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE businesses SET settings_json = ?1, updated_at = ?2 WHERE business_id = ?3",
        rusqlite::params![settings_json, now, user.business_id],
    )?;

    crate::audit::record(
        &tx,
        &user.business_id,
        Some(&user.user_id),
        "remove_business_logo",
        "business",
        Some(&user.business_id),
        "{}",
    )?;
    tx.commit()?;

    fetch_business(&conn, &user.business_id).map(Json)
}
