//! Local OCR engine adapter and deterministic bill parser
//! (Phase 10 / V0.2 Purchase Photo/OCR, spec §6.5, §7, §8).
//!
//! Architectural boundaries:
//! 1. Isolated from core billing & inventory: OCR only proposes raw candidate
//!    strings and values. It never writes directly to financial totals columns
//!    or the inventory ledger.
//! 2. Human-in-the-loop: All extracted fields are reviewed, corrected, and
//!    matched by staff in the frontend before approval.
//! 3. Deterministic parsing: Uses zero-dependency string heuristics for Indian invoice
//!    structures (GSTIN, Invoice No, Date, HSN, Tax Slabs, Tabular Item Lines).

use crate::error::ApiError;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// `engine_version` reported when canned development data is returned instead of reading
/// the image. API clients use it to flag such results as fake.
pub const DEV_FIXTURE_ENGINE_VERSION: &str = "artha_ocr_dev_fixture_v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OcrToken {
    pub text: String,
    pub confidence: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub box_points: Option<[[f32; 2]; 4]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrResultPayload {
    pub engine_version: String,
    pub confidence: f32,
    pub tokens: Vec<OcrToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParsedBillItem {
    pub product_name_snapshot: String,
    pub quantity_milli: i64,
    pub price_paise: i64,
    pub gst_rate_bps: i64,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParsedBill {
    pub invoice_no: Option<String>,
    pub invoice_no_confidence: f32,
    pub invoice_date: Option<String>,
    pub invoice_date_confidence: f32,
    pub supplier_name: Option<String>,
    pub items: Vec<ParsedBillItem>,
    pub stated_total_paise: Option<i64>,
    pub average_confidence: f32,
}

/// Normalizes an Indian date string (`DD/MM/YYYY`, `DD-MM-YYYY`, `YYYY-MM-DD`)
/// to ISO-8601 `YYYY-MM-DD`.
pub fn normalize_date(raw: &str) -> Option<String> {
    let clean = raw.trim();
    let parts: Vec<&str> = clean.split(['/', '-', '.']).collect();
    if parts.len() != 3 {
        return None;
    }

    // YYYY-MM-DD
    if parts[0].len() == 4 && (1..=2).contains(&parts[1].len()) && (1..=2).contains(&parts[2].len())
    {
        let y: u32 = parts[0].parse().ok()?;
        let m: u32 = parts[1].parse().ok()?;
        let d: u32 = parts[2].parse().ok()?;
        if (1..=12).contains(&m) && (1..=31).contains(&d) {
            return Some(format!("{y:04}-{m:02}-{d:02}"));
        }
    }

    // DD-MM-YYYY or DD/MM/YYYY
    if (1..=2).contains(&parts[0].len()) && (1..=2).contains(&parts[1].len()) && parts[2].len() == 4
    {
        let d: u32 = parts[0].parse().ok()?;
        let m: u32 = parts[1].parse().ok()?;
        let y: u32 = parts[2].parse().ok()?;
        if (1..=12).contains(&m) && (1..=31).contains(&d) {
            return Some(format!("{y:04}-{m:02}-{d:02}"));
        }
    }

    None
}

/// Parses an Indian currency / price amount string to integer paise.
/// E.g. "150.00" -> 15000, "1,250.50" -> 125050, "45" -> 4500.
pub fn parse_amount_to_paise(raw: &str) -> Option<i64> {
    let cleaned: String = raw
        .chars()
        .filter(|&c| c.is_ascii_digit() || c == '.')
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    let parts: Vec<&str> = cleaned.split('.').collect();
    // Checked arithmetic: OCR text is untrusted, and an unchecked `rupees * 100` panics
    // in debug builds and silently wraps to a wrong amount in release builds.
    if parts.len() == 1 {
        let rupees: i64 = parts[0].parse().ok()?;
        rupees.checked_mul(100)
    } else if parts.len() == 2 {
        let rupees: i64 = parts[0].parse().ok()?;
        let paise_part = parts[1];
        let paise: i64 = match paise_part.len() {
            0 => 0,
            1 => paise_part.parse::<i64>().ok()? * 10,
            2 => paise_part.parse::<i64>().ok()?,
            _ => paise_part[..2].parse::<i64>().ok()?,
        };
        rupees.checked_mul(100)?.checked_add(paise)
    } else {
        None
    }
}

/// Parses standard Indian GST slab string (e.g. "18%", "5", "12 %") to basis points.
pub fn parse_gst_rate_bps(raw: &str) -> Option<i64> {
    let clean = raw.trim().trim_end_matches('%').trim();
    match clean {
        "0" | "0.0" => Some(0),
        "5" | "5.0" => Some(500),
        "12" | "12.0" => Some(1200),
        "18" | "18.0" => Some(1800),
        "28" | "28.0" => Some(2800),
        _ => None,
    }
}

/// Extracts an invoice number candidate from an invoice header text line.
fn extract_invoice_no(text: &str) -> Option<String> {
    // ASCII-only folding keeps byte offsets identical to `text`. A full Unicode
    // `to_lowercase()` can change the byte length (e.g. U+0130, U+212A), so an offset
    // found in `lower` would then index `text` out of bounds or off a char boundary.
    // OCR output is untrusted input; the keywords below are ASCII anyway.
    let lower = text.to_ascii_lowercase();
    let keywords = [
        "invoice no",
        "invoice #",
        "invoice number",
        "bill no",
        "bill number",
        "bill #",
        "inv no",
        "inv #",
    ];
    for kw in &keywords {
        if let Some(pos) = lower.find(kw) {
            let rest = &text[pos + kw.len()..];
            let candidate: String = rest
                .trim_start_matches(|c: char| c == ':' || c == '.' || c == '#' || c.is_whitespace())
                .split_whitespace()
                .next()
                .unwrap_or("")
                .chars()
                .filter(|&c| c.is_alphanumeric() || c == '-' || c == '/' || c == '_')
                .collect();
            if candidate.len() >= 2 && !candidate.to_lowercase().contains("date") {
                return Some(candidate);
            }
        }
    }
    None
}

/// Deterministic parser that turns raw OCR tokens into structured purchase draft candidates.
pub fn parse_bill_tokens(tokens: &[OcrToken]) -> ParsedBill {
    if tokens.is_empty() {
        return ParsedBill {
            invoice_no: None,
            invoice_no_confidence: 0.0,
            invoice_date: None,
            invoice_date_confidence: 0.0,
            supplier_name: None,
            items: Vec::new(),
            stated_total_paise: None,
            average_confidence: 0.0,
        };
    }

    let avg_confidence: f32 =
        tokens.iter().map(|t| t.confidence).sum::<f32>() / tokens.len() as f32;

    let mut invoice_no = None;
    let mut invoice_no_confidence = 0.0;
    let mut invoice_date = None;
    let mut invoice_date_confidence = 0.0;
    let mut supplier_name = None;
    let mut stated_total_paise = None;

    // 1. Header scan: invoice_no, date, supplier, totals
    for (idx, token) in tokens.iter().enumerate() {
        let text = token.text.trim();
        let lower = text.to_lowercase();

        // Invoice number detection
        if invoice_no.is_none() {
            if let Some(candidate) = extract_invoice_no(text) {
                invoice_no = Some(candidate);
                invoice_no_confidence = token.confidence;
            } else if idx + 1 < tokens.len()
                && (lower == "invoice no" || lower == "bill no" || lower == "inv no")
            {
                let candidate = tokens[idx + 1].text.trim().to_string();
                if !candidate.is_empty() {
                    invoice_no = Some(candidate);
                    invoice_no_confidence = tokens[idx + 1].confidence;
                }
            }
        }

        // Date detection
        if invoice_date.is_none() {
            for word in text.split_whitespace() {
                let cleaned = word.trim_matches(|c: char| {
                    !c.is_alphanumeric() && c != '/' && c != '-' && c != '.'
                });
                if let Some(normalized) = normalize_date(cleaned) {
                    invoice_date = Some(normalized);
                    invoice_date_confidence = token.confidence;
                    break;
                }
            }
        }

        // Total amount detection
        if stated_total_paise.is_none()
            && (lower.contains("grand total")
                || lower.contains("total amount")
                || lower.contains("net payable"))
        {
            for word in text.split_whitespace().rev() {
                if let Some(paise) = parse_amount_to_paise(word) {
                    if paise > 0 {
                        stated_total_paise = Some(paise);
                        break;
                    }
                }
            }
        }
    }

    // Heuristic supplier name: first prominent uppercase line that is not a generic header
    for token in tokens.iter().take(5) {
        let t = token.text.trim();
        let lower = t.to_lowercase();
        if t.len() > 3
            && !lower.contains("invoice")
            && !lower.contains("bill")
            && !lower.contains("date")
            && !lower.contains("gstin")
            && !lower.contains("tax invoice")
            && !lower.contains("cash memo")
        {
            supplier_name = Some(t.to_string());
            break;
        }
    }

    // 2. Tabular Line Items Detection
    let mut items = Vec::new();

    for token in tokens {
        let text = token.text.trim();
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.len() >= 3 {
            // Check if last word is a tax rate (e.g. "5%", "18%", "0%")
            let (gst_rate_bps, price_str, qty_str, name_words) =
                if words.last().unwrap().ends_with('%') {
                    if words.len() >= 4 {
                        let gst = parse_gst_rate_bps(words[words.len() - 1]).unwrap_or(0);
                        let price = words[words.len() - 2];
                        let qty = words[words.len() - 3];
                        let name = &words[..words.len() - 3];
                        (gst, price, qty, name)
                    } else {
                        continue;
                    }
                } else {
                    let price = words[words.len() - 1];
                    let qty = words[words.len() - 2];
                    let name = &words[..words.len() - 2];
                    (0, price, qty, name)
                };

            if let Some(price_paise) = parse_amount_to_paise(price_str) {
                // Parse quantity (clean out units like kg, ltr, pcs, etc.)
                let cleaned_qty: String = qty_str
                    .chars()
                    .filter(|&c| c.is_ascii_digit() || c == '.')
                    .collect();
                if let Ok(qty_f) = cleaned_qty.parse::<f64>() {
                    let name = name_words.join(" ");
                    if !name.is_empty() && price_paise > 0 && qty_f > 0.0 {
                        items.push(ParsedBillItem {
                            product_name_snapshot: name,
                            quantity_milli: (qty_f * 1000.0).round() as i64,
                            price_paise,
                            gst_rate_bps,
                            confidence: token.confidence,
                        });
                    }
                }
            }
        }
    }

    ParsedBill {
        invoice_no,
        invoice_no_confidence,
        invoice_date,
        invoice_date_confidence,
        supplier_name,
        items,
        stated_total_paise,
        average_confidence: avg_confidence,
    }
}

pub fn fixture_tokens() -> Vec<OcrToken> {
    vec![
        OcrToken {
            text: "SHREE GANESH ENTERPRISES".to_string(),
            confidence: 0.98,
            box_points: None,
        },
        OcrToken {
            text: "GSTIN: 27AABCU9603R1ZM".to_string(),
            confidence: 0.96,
            box_points: None,
        },
        OcrToken {
            text: "Tax Invoice".to_string(),
            confidence: 0.95,
            box_points: None,
        },
        OcrToken {
            text: "Invoice No: INV-2026-089".to_string(),
            confidence: 0.97,
            box_points: None,
        },
        OcrToken {
            text: "Date: 12/10/2026".to_string(),
            confidence: 0.96,
            box_points: None,
        },
        OcrToken {
            text: "Basmati Rice 5kg 350.00 5%".to_string(),
            confidence: 0.92,
            box_points: None,
        },
        OcrToken {
            text: "Sunflower Oil 1L 140.00 5%".to_string(),
            confidence: 0.91,
            box_points: None,
        },
        OcrToken {
            text: "Wheat Flour 10kg 420.00 0%".to_string(),
            confidence: 0.93,
            box_points: None,
        },
        OcrToken {
            text: "Grand Total: 910.00".to_string(),
            confidence: 0.95,
            box_points: None,
        },
    ]
}

fn truncate_for_log(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// Executes OCR processing on an image file.
///
/// Priority & Subprocess Management:
/// 1. If an external OCR command (`cmd_opt`) is provided, spawns that child process using
///    Tokio's async process API with `kill_on_drop(true)` and bounds execution by `timeout_duration`.
///    On timeout the future is dropped, which kills the *direct* child only. Processes that
///    child itself started (for example the Python interpreter behind a wrapper script) are
///    NOT killed and keep running until they exit on their own. Killing the whole process
///    tree needs platform-specific code (process groups on Unix, a Job Object on Windows).
/// 2. If no command is configured:
///    - In Production (`Environment::Production`): Fails with an actionable `ApiError::BadRequest`
///      indicating that local OCR is not configured and advising manual entry. It NEVER returns
///      dummy fixture data in production.
///    - In Development / Tests (`Environment::Development`): Returns deterministic fixture
///      tokens for testing and offline local development.
pub async fn execute_ocr(
    image_path: &Path,
    cmd_opt: Option<&str>,
    environment: crate::config::Environment,
    timeout_duration: std::time::Duration,
) -> Result<(OcrResultPayload, ParsedBill), ApiError> {
    if !image_path.exists() {
        return Err(ApiError::NotFound("Image file not found on disk".into()));
    }

    if let Some(cmd) = cmd_opt {
        let path_str = image_path.to_string_lossy().to_string();
        let child = tokio::process::Command::new(cmd)
            .arg(&path_str)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                // Detail goes to the server log only: the command path and OS error
                // would otherwise reveal the server's filesystem layout to API clients.
                tracing::error!(command = %cmd, error = %e, "failed to start the configured OCR engine");
                ApiError::BadRequest(
                    "Failed to start the configured OCR engine. Check that ARTHA_PADDLE_OCR_CMD points to an executable file, or enter the bill details manually."
                        .into(),
                )
            })?;

        let output = match tokio::time::timeout(timeout_duration, child.wait_with_output()).await {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => {
                return Err(ApiError::Internal(format!("OCR process I/O error: {e}")));
            }
            Err(_) => {
                return Err(ApiError::BadRequest(format!(
                    "OCR processing timed out after {} seconds",
                    timeout_duration.as_secs()
                )));
            }
        };

        if !output.status.success() {
            // Engine stderr (tracebacks, file paths) is logged, never returned to the client.
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(
                exit_code = ?output.status.code(),
                stderr = %truncate_for_log(&stderr, 2000),
                "OCR engine exited with an error"
            );
            let code = output
                .status
                .code()
                .map_or_else(|| "unknown".to_string(), |c| c.to_string());
            return Err(ApiError::BadRequest(format!(
                "The OCR engine failed (exit code {code}). You can enter the bill details manually."
            )));
        }

        let raw_tokens: Vec<OcrToken> = serde_json::from_slice(&output.stdout).map_err(|e| {
            tracing::warn!(error = %e, "OCR engine output was not valid token JSON");
            ApiError::BadRequest(
                "The OCR engine returned output that could not be read. You can enter the bill details manually."
                    .into(),
            )
        })?;

        let parsed = parse_bill_tokens(&raw_tokens);
        let payload = OcrResultPayload {
            engine_version: "paddleocr_cli_v1".to_string(),
            confidence: parsed.average_confidence,
            tokens: raw_tokens,
        };
        return Ok((payload, parsed));
    }

    // No OCR command configured:
    if environment == crate::config::Environment::Production {
        return Err(ApiError::BadRequest(
            "Local OCR engine is not configured on this server (ARTHA_PADDLE_OCR_CMD is not set). Please configure an OCR command or enter bill details manually.".into(),
        ));
    }

    // Default built-in OCR processor for development & tests:
    let tokens = fixture_tokens();
    let parsed = parse_bill_tokens(&tokens);
    let payload = OcrResultPayload {
        engine_version: DEV_FIXTURE_ENGINE_VERSION.to_string(),
        confidence: parsed.average_confidence,
        tokens,
    };

    Ok((payload, parsed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_date_formats() {
        assert_eq!(normalize_date("12/10/2026"), Some("2026-10-12".to_string()));
        assert_eq!(normalize_date("05-09-2025"), Some("2025-09-05".to_string()));
        assert_eq!(normalize_date("2026-11-23"), Some("2026-11-23".to_string()));
        assert_eq!(normalize_date("invalid-date"), None);
    }

    #[test]
    fn test_parse_amount_to_paise() {
        assert_eq!(parse_amount_to_paise("150.00"), Some(15000));
        assert_eq!(parse_amount_to_paise("₹ 1,250.50"), Some(125050));
        assert_eq!(parse_amount_to_paise("45"), Some(4500));
        assert_eq!(parse_amount_to_paise("0.75"), Some(75));
        assert_eq!(parse_amount_to_paise("invalid"), None);
    }

    #[test]
    fn test_parse_gst_rate_bps() {
        assert_eq!(parse_gst_rate_bps("0%"), Some(0));
        assert_eq!(parse_gst_rate_bps("5%"), Some(500));
        assert_eq!(parse_gst_rate_bps("12"), Some(1200));
        assert_eq!(parse_gst_rate_bps("18%"), Some(1800));
        assert_eq!(parse_gst_rate_bps("28%"), Some(2800));
        assert_eq!(parse_gst_rate_bps("33%"), None);
    }

    #[test]
    fn test_parse_bill_tokens_end_to_end() {
        let tokens = vec![
            OcrToken {
                text: "FARM FRESH DISTRIBUTORS".to_string(),
                confidence: 0.99,
                box_points: None,
            },
            OcrToken {
                text: "Bill No: BL-9842".to_string(),
                confidence: 0.95,
                box_points: None,
            },
            OcrToken {
                text: "Date: 15/08/2026".to_string(),
                confidence: 0.96,
                box_points: None,
            },
            OcrToken {
                text: "Organic Sugar 2kg 90.00 5%".to_string(),
                confidence: 0.92,
                box_points: None,
            },
            OcrToken {
                text: "Pure Ghee 1L 550.00 12%".to_string(),
                confidence: 0.90,
                box_points: None,
            },
            OcrToken {
                text: "Grand Total: 640.00".to_string(),
                confidence: 0.97,
                box_points: None,
            },
        ];

        let parsed = parse_bill_tokens(&tokens);
        assert_eq!(parsed.invoice_no, Some("BL-9842".to_string()));
        assert_eq!(parsed.invoice_date, Some("2026-08-15".to_string()));
        assert_eq!(
            parsed.supplier_name,
            Some("FARM FRESH DISTRIBUTORS".to_string())
        );
        assert_eq!(parsed.stated_total_paise, Some(64000));
        assert_eq!(parsed.items.len(), 2);

        assert_eq!(parsed.items[0].product_name_snapshot, "Organic Sugar");
        assert_eq!(parsed.items[0].quantity_milli, 2000);
        assert_eq!(parsed.items[0].price_paise, 9000);
        assert_eq!(parsed.items[0].gst_rate_bps, 500);

        assert_eq!(parsed.items[1].product_name_snapshot, "Pure Ghee");
        assert_eq!(parsed.items[1].quantity_milli, 1000);
        assert_eq!(parsed.items[1].price_paise, 55000);
        assert_eq!(parsed.items[1].gst_rate_bps, 1200);
    }

    #[tokio::test]
    async fn test_execute_ocr_production_without_cmd_fails_clearly() {
        let temp_dir = std::env::temp_dir();
        let image_path = temp_dir.join(format!("test_img_prod_{}.png", std::process::id()));
        std::fs::write(&image_path, b"test-image-content").unwrap();

        let result = execute_ocr(
            &image_path,
            None,
            crate::config::Environment::Production,
            std::time::Duration::from_secs(30),
        )
        .await;

        let _ = std::fs::remove_file(&image_path);

        match result {
            Err(ApiError::BadRequest(msg)) => {
                assert!(
                    msg.contains("Local OCR engine is not configured on this server"),
                    "Expected actionable error message, got: {msg}"
                );
            }
            other => panic!("Expected ApiError::BadRequest, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_execute_ocr_development_without_cmd_returns_fixtures() {
        let temp_dir = std::env::temp_dir();
        let image_path = temp_dir.join(format!("test_img_dev_{}.png", std::process::id()));
        std::fs::write(&image_path, b"test-image-content").unwrap();

        let result = execute_ocr(
            &image_path,
            None,
            crate::config::Environment::Development,
            std::time::Duration::from_secs(30),
        )
        .await;

        let _ = std::fs::remove_file(&image_path);

        let (payload, parsed) = result.expect("Dev mode should succeed with fixtures");
        assert_eq!(payload.engine_version, "artha_ocr_dev_fixture_v1");
        assert_eq!(parsed.invoice_no, Some("INV-2026-089".to_string()));
        assert_eq!(parsed.items.len(), 3);
    }

    #[tokio::test]
    async fn test_execute_ocr_missing_file_returns_not_found() {
        let nonexistent = Path::new("nonexistent_artha_test_image.png");
        let result = execute_ocr(
            nonexistent,
            None,
            crate::config::Environment::Development,
            std::time::Duration::from_secs(30),
        )
        .await;

        assert!(matches!(result, Err(ApiError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_execute_ocr_timeout_terminates_and_returns_error() {
        let temp_dir = std::env::temp_dir();
        let image_path = temp_dir.join(format!("test_img_timeout_{}.png", std::process::id()));
        std::fs::write(&image_path, b"test-image-content").unwrap();

        #[cfg(windows)]
        let (script_path, cmd) = {
            let script = temp_dir.join(format!("test_hang_{}.cmd", std::process::id()));
            std::fs::write(&script, "@ping -n 5 127.0.0.1 >nul\r\n").unwrap();
            let path_str = script.to_string_lossy().to_string();
            (Some(script), path_str)
        };
        #[cfg(not(windows))]
        let (script_path, cmd) = {
            use std::os::unix::fs::PermissionsExt;
            let script = temp_dir.join(format!("test_hang_{}.sh", std::process::id()));
            std::fs::write(&script, "#!/bin/sh\nsleep 5\n").unwrap();
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
            let path_str = script.to_string_lossy().to_string();
            (Some(script), path_str)
        };

        let result = execute_ocr(
            &image_path,
            Some(&cmd),
            crate::config::Environment::Development,
            std::time::Duration::from_millis(150),
        )
        .await;

        let _ = std::fs::remove_file(&image_path);
        if let Some(script) = script_path {
            let _ = std::fs::remove_file(script);
        }

        match result {
            Err(ApiError::BadRequest(msg)) => {
                assert!(
                    msg.contains("timed out"),
                    "Expected timeout message, got: {msg}"
                );
            }
            other => panic!("Expected timeout ApiError::BadRequest, got: {other:?}"),
        }
    }

    fn tok(text: &str) -> OcrToken {
        OcrToken {
            text: text.to_string(),
            confidence: 0.9,
            box_points: None,
        }
    }

    #[test]
    fn invoice_number_extraction_survives_unicode_case_folding() {
        // U+0130 lowercases to more bytes than it occupies: the old offset math then
        // sliced `text` out of bounds and panicked.
        let parsed = parse_bill_tokens(&[tok("\u{130}\u{130}\u{130}\u{130} Bill No: BL-77")]);
        assert_eq!(parsed.invoice_no, Some("BL-77".to_string()));

        // U+212A (Kelvin sign) lowercases to fewer bytes: the old code returned "voice".
        let parsed = parse_bill_tokens(&[tok("\u{212A}\u{212A} Invoice No: AB-12")]);
        assert_eq!(parsed.invoice_no, Some("AB-12".to_string()));

        // A keyword-only line padded with such characters must not panic.
        let _ = parse_bill_tokens(&[tok(
            "\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130} bill no",
        )]);
    }

    #[test]
    fn parse_amount_to_paise_rejects_values_that_overflow_i64() {
        assert_eq!(
            parse_amount_to_paise("92233720368547758"),
            Some(9_223_372_036_854_775_800)
        );
        assert_eq!(
            parse_amount_to_paise("92233720368547758.07"),
            Some(i64::MAX)
        );
        assert_eq!(parse_amount_to_paise("92233720368547759"), None);
        assert_eq!(parse_amount_to_paise("92233720368547758.99"), None);
        assert_eq!(parse_amount_to_paise("999999999999999999"), None);
    }

    #[test]
    fn hostile_amounts_in_a_bill_do_not_panic_the_parser() {
        let parsed = parse_bill_tokens(&[
            tok("Grand Total: 999999999999999999"),
            tok("Rice 5 999999999999999999 5%"),
        ]);
        assert_eq!(parsed.stated_total_paise, None);
        assert!(parsed.items.is_empty());
    }

    /// Writes a small executable script for the current platform and returns its path.
    fn write_script(tag: &str, windows_body: &str, unix_body: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        #[cfg(windows)]
        {
            let _ = unix_body;
            let p = dir.join(format!("artha_ocr_{tag}_{}.cmd", std::process::id()));
            std::fs::write(&p, windows_body).unwrap();
            p
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = windows_body;
            let p = dir.join(format!("artha_ocr_{tag}_{}.sh", std::process::id()));
            std::fs::write(&p, unix_body).unwrap();
            let mut perms = std::fs::metadata(&p).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&p, perms).unwrap();
            p
        }
    }

    async fn run_script_engine(
        tag: &str,
        windows_body: &str,
        unix_body: &str,
    ) -> Result<(OcrResultPayload, ParsedBill), ApiError> {
        let image =
            std::env::temp_dir().join(format!("artha_ocr_{tag}_{}.png", std::process::id()));
        std::fs::write(&image, b"test-image-content").unwrap();
        let script = write_script(tag, windows_body, unix_body);
        let result = execute_ocr(
            &image,
            Some(&script.to_string_lossy()),
            crate::config::Environment::Development,
            std::time::Duration::from_secs(20),
        )
        .await;
        let _ = std::fs::remove_file(&image);
        let _ = std::fs::remove_file(&script);
        result
    }

    #[tokio::test]
    async fn spawn_failure_does_not_leak_the_command_path_or_os_error() {
        let image =
            std::env::temp_dir().join(format!("artha_ocr_spawn_{}.png", std::process::id()));
        std::fs::write(&image, b"test-image-content").unwrap();
        let result = execute_ocr(
            &image,
            Some("/definitely/missing/dir/ocr-engine"),
            crate::config::Environment::Development,
            std::time::Duration::from_secs(5),
        )
        .await;
        let _ = std::fs::remove_file(&image);

        match result {
            Err(ApiError::BadRequest(msg)) => {
                assert!(
                    msg.contains("Failed to start the configured OCR engine"),
                    "{msg}"
                );
                assert!(!msg.contains("definitely"), "command path leaked: {msg}");
                assert!(!msg.contains("os error"), "OS error leaked: {msg}");
            }
            other => panic!("Expected ApiError::BadRequest, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn engine_failure_does_not_leak_stderr_or_paths() {
        let result = run_script_engine(
            "fail",
            "@echo Traceback File C:\\secret\\paddle\\engine.py 1>&2\r\n@exit /b 3\r\n",
            "#!/bin/sh\necho 'Traceback File /secret/paddle/engine.py' >&2\nexit 3\n",
        )
        .await;

        match result {
            Err(ApiError::BadRequest(msg)) => {
                assert!(msg.contains("exit code 3"), "{msg}");
                assert!(!msg.contains("secret"), "path leaked: {msg}");
                assert!(!msg.contains("engine.py"), "path leaked: {msg}");
                assert!(!msg.contains("Traceback"), "stderr leaked: {msg}");
            }
            other => panic!("Expected ApiError::BadRequest, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn unreadable_engine_output_returns_a_generic_error() {
        let result = run_script_engine(
            "badjson",
            "@echo not-json\r\n",
            "#!/bin/sh\necho not-json\n",
        )
        .await;

        match result {
            Err(ApiError::BadRequest(msg)) => {
                assert!(msg.contains("could not be read"), "{msg}");
                assert!(!msg.contains("expected"), "parser internals leaked: {msg}");
            }
            other => panic!("Expected ApiError::BadRequest, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_real_engine_result_is_not_flagged_as_dev_fixture() {
        let (payload, parsed) =
            run_script_engine("okjson", "@echo []\r\n", "#!/bin/sh\necho '[]'\n")
                .await
                .expect("valid engine output must parse");
        assert_eq!(payload.engine_version, "paddleocr_cli_v1");
        assert_ne!(payload.engine_version, DEV_FIXTURE_ENGINE_VERSION);
        assert!(parsed.items.is_empty());
    }
}
