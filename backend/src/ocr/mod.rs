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

use crate::config::Environment;
use crate::error::ApiError;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

pub mod runner;

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
    /// Suggested billed quantity in thousandths. Only meaningful as a *claim about the
    /// bill* when `quantity_certain` is true; otherwise it is a placeholder (1000 = one
    /// pack, or the number found next to a multiplier) that a person must confirm.
    pub quantity_milli: i64,
    /// False when the text could not reliably distinguish the billed quantity from a
    /// pack size or unit (for example `5kg`), so the suggestion must be reviewed.
    pub quantity_certain: bool,
    /// Pack size / unit text seen next to the quantity position, e.g. `5kg`, `1L`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pack_size: Option<String>,
    /// Plain-language reason the quantity needs review.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub quantity_note: Option<String>,
    pub price_paise: i64,
    pub gst_rate_bps: i64,
    pub confidence: f32,
}

/// Largest quantity accepted from OCR text: 1,000,000 units (in thousandths).
const MAX_QUANTITY_MILLI: i64 = 1_000_000_000;
/// Largest unit price accepted from OCR text: 1e11 paise (Rs 100 crore). With the quantity
/// cap, a line total stays far below `i64::MAX`.
const MAX_ITEM_PRICE_PAISE: i64 = 100_000_000_000;

/// Units that count things. `12 pcs` means twelve of them.
const COUNT_UNITS: &[&str] = &[
    "pc", "pcs", "piece", "pieces", "no", "nos", "unit", "units", "pkt", "pkts", "packet",
    "packets", "box", "boxes", "btl", "btls", "bottle", "bottles", "bag", "bags", "doz", "dozen",
    "ctn", "carton", "cartons", "set", "sets", "pair", "pairs", "jar", "jars", "tin", "tins",
    "can", "cans",
];

/// Units that measure weight / volume / length. Next to a product name (`Rice 5kg`) they
/// usually describe the *pack*, not how many packs were billed.
const MEASURE_UNITS: &[&str] = &[
    "kg", "kgs", "g", "gm", "gms", "gram", "grams", "mg", "l", "lt", "ltr", "ltrs", "litre",
    "litres", "liter", "liters", "ml", "oz", "lb", "lbs", "m", "mtr", "mtrs", "cm",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum UnitKind {
    Count,
    Measure,
}

fn classify_unit(unit: &str) -> Option<UnitKind> {
    let u = unit.to_ascii_lowercase();
    if COUNT_UNITS.contains(&u.as_str()) {
        Some(UnitKind::Count)
    } else if MEASURE_UNITS.contains(&u.as_str()) {
        Some(UnitKind::Measure)
    } else {
        None
    }
}

/// Parses a plain decimal quantity (`3`, `2.5`, `.75`) into thousandths using integer
/// arithmetic only. Anything not exactly representable (more than three decimals), zero,
/// negative, or above [`MAX_QUANTITY_MILLI`] is rejected rather than rounded.
fn parse_quantity_milli(raw: &str) -> Option<i64> {
    let (whole, frac) = raw.split_once('.').unwrap_or((raw, ""));
    if whole.is_empty() && frac.is_empty() {
        return None;
    }
    if !whole.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if whole.len() > 9 || frac.len() > 3 {
        return None;
    }
    let w: i64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
    let mut f: i64 = if frac.is_empty() { 0 } else { frac.parse().ok()? };
    for _ in frac.len()..3 {
        f *= 10;
    }
    let milli = w.checked_mul(1000)?.checked_add(f)?;
    (1..=MAX_QUANTITY_MILLI).contains(&milli).then_some(milli)
}

/// Splits `5kg` / `1.5L` / `12pcs` into (`5`, `kg`). The unit must be letters only.
fn split_number_unit(token: &str) -> Option<(&str, &str)> {
    let idx = token.find(|c: char| c.is_ascii_alphabetic())?;
    let (num, unit) = token.split_at(idx);
    let unit = unit.trim_end_matches('.');
    if num.is_empty() || unit.is_empty() || !unit.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    Some((num, unit))
}

/// What the quantity position of an invoice line turned out to say.
struct QuantityReading {
    milli: i64,
    certain: bool,
    pack_size: Option<String>,
    note: Option<String>,
}

impl QuantityReading {
    fn certain(milli: i64) -> Self {
        Self {
            milli,
            certain: true,
            pack_size: None,
            note: None,
        }
    }

    /// A pack/unit was seen but the billed quantity was not stated: suggest one pack.
    fn pack_only(pack: String) -> Self {
        let note = format!(
            "\"{pack}\" looks like a pack size, so the billed quantity was not found. Check the bill and enter the number of packs."
        );
        Self {
            milli: 1000,
            certain: false,
            pack_size: Some(pack),
            note: Some(note),
        }
    }

    /// A number such as `100.00` sits where the quantity should be.
    fn amount_like(milli: i64, raw: &str) -> Self {
        Self {
            milli,
            certain: false,
            pack_size: None,
            note: Some(format!(
                "\"{raw}\" looks like an amount, not a quantity (the line may have extra price columns). Check the bill."
            )),
        }
    }

    /// `2 x 1kg`: a count and a pack, but the text does not say whether the price is per
    /// pack or for the line, so a person must confirm.
    fn multiplier(count_milli: i64, pack: String) -> Self {
        let note = format!(
            "Read as {} x \"{pack}\". Confirm the number of packs and that the price is per pack.",
            fmt_milli(count_milli)
        );
        Self {
            milli: count_milli,
            certain: false,
            pack_size: Some(pack),
            note: Some(note),
        }
    }
}

fn fmt_milli(milli: i64) -> String {
    if milli % 1000 == 0 {
        (milli / 1000).to_string()
    } else {
        format!("{}.{:03}", milli / 1000, milli % 1000)
            .trim_end_matches('0')
            .to_string()
    }
}

/// Interprets the words just before the price, returning the reading and how many words
/// it used. Only an explicitly labelled quantity, a bare number in the quantity column, or
/// a count unit (`12 pcs`) is *certain*; weight/volume packs and multiplier forms are not.
fn read_quantity(rest: &[&str]) -> Option<(QuantityReading, usize)> {
    let n = rest.len();
    let last = *rest.last()?;
    let prev = if n >= 2 { Some(rest[n - 2]) } else { None };

    // `Qty 3`, `Qty: 3`, `Quantity 3`
    if let Some(p) = prev {
        let p = p.to_ascii_lowercase();
        if matches!(p.trim_end_matches([':', '.']), "qty" | "quantity") {
            if let Some(q) = parse_quantity_milli(last) {
                return Some((QuantityReading::certain(q), 2));
            }
        }
    }
    // `Qty:3`
    let lower_last = last.to_ascii_lowercase();
    if let Some(after) = lower_last.strip_prefix("qty") {
        if let Some(q) = parse_quantity_milli(after.trim_start_matches([':', '.', '-', '='])) {
            return Some((QuantityReading::certain(q), 1));
        }
    }

    // `2 x 1kg` / `2 X 500ml`
    if n >= 3 && matches!(rest[n - 2], "x" | "X" | "\u{d7}") {
        if let (Some(count), Some((_, unit))) =
            (parse_quantity_milli(rest[n - 3]), split_number_unit(last))
        {
            if classify_unit(unit).is_some() {
                return Some((QuantityReading::multiplier(count, last.to_string()), 3));
            }
        }
    }
    // `2x1kg`
    if let Some(pos) = last.find(['x', 'X', '\u{d7}']) {
        let (left, right) = last.split_at(pos);
        let right = &right[last[pos..].chars().next().map_or(1, char::len_utf8)..];
        if let (Some(count), Some((_, unit))) =
            (parse_quantity_milli(left), split_number_unit(right))
        {
            if classify_unit(unit).is_some() {
                return Some((QuantityReading::multiplier(count, right.to_string()), 1));
            }
        }
    }

    // `12 pcs` / `5 kg`: number and unit as two words
    if let Some(p) = prev {
        if last.bytes().all(|b| b.is_ascii_alphabetic() || b == b'.') {
            if let (Some(q), Some(kind)) = (
                parse_quantity_milli(p),
                classify_unit(last.trim_end_matches('.')),
            ) {
                return Some(match kind {
                    UnitKind::Count => (QuantityReading::certain(q), 2),
                    UnitKind::Measure => {
                        (QuantityReading::pack_only(format!("{p}{}", last.trim_end_matches('.'))), 2)
                    }
                });
            }
        }
    }

    // `12pcs` / `5kg` / `1L`: number glued to a unit
    if let Some((num, unit)) = split_number_unit(last) {
        if let (Some(q), Some(kind)) = (parse_quantity_milli(num), classify_unit(unit)) {
            return Some(match kind {
                UnitKind::Count => (QuantityReading::certain(q), 1),
                UnitKind::Measure => (QuantityReading::pack_only(last.to_string()), 1),
            });
        }
        return None;
    }

    // A bare number in the quantity column. Exactly two decimals (`100.00`) reads like
    // an amount, which happens when the line carries extra price columns (rate and line
    // total), so that is not trusted as a quantity.
    let q = parse_quantity_milli(last)?;
    let looks_like_amount = last.split_once('.').is_some_and(|(_, frac)| frac.len() == 2);
    if looks_like_amount {
        Some((QuantityReading::amount_like(q, last), 1))
    } else {
        Some((QuantityReading::certain(q), 1))
    }
}

/// Lines that summarise the bill rather than list a product.
fn is_summary_line(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["total", "discount", "round", "balance", "cgst", "sgst", "igst"]
        .iter()
        .any(|k| lower.contains(k))
}

/// Parses one OCR line of the form `<name> <quantity section> <unit price> [<gst>%]`.
/// Returns `None` for anything that does not fit; never guesses a quantity it cannot read.
fn parse_item_line(text: &str, confidence: f32) -> Option<ParsedBillItem> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 3 {
        return None;
    }

    // Optional trailing tax rate such as `5%`.
    let (gst_rate_bps, body) = if words.last()?.ends_with('%') {
        if words.len() < 4 {
            return None;
        }
        (
            parse_gst_rate_bps(words[words.len() - 1]).unwrap_or(0),
            &words[..words.len() - 1],
        )
    } else {
        (0, &words[..])
    };

    let price_paise = parse_amount_to_paise(body.last()?)?;
    if price_paise <= 0 || price_paise > MAX_ITEM_PRICE_PAISE {
        return None;
    }
    let rest = &body[..body.len() - 1];

    let (reading, used) = read_quantity(rest)?;
    let name = rest[..rest.len() - used].join(" ");
    if name.is_empty() || is_summary_line(&name) {
        return None;
    }

    Some(ParsedBillItem {
        product_name_snapshot: name,
        quantity_milli: reading.milli,
        quantity_certain: reading.certain,
        pack_size: reading.pack_size,
        quantity_note: reading.note,
        price_paise,
        gst_rate_bps,
        confidence,
    })
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

/// Earliest and latest year accepted for an invoice date. Wide enough for any real bill,
/// narrow enough to reject OCR noise such as `00/00/0000` or `12/10/9026`.
const MIN_INVOICE_YEAR: u32 = 1990;
const MAX_INVOICE_YEAR: u32 = 2100;

/// True for a real calendar date (month lengths and leap years respected) in the
/// accepted year range.
pub fn is_valid_calendar_date(y: u32, m: u32, d: u32) -> bool {
    if !(MIN_INVOICE_YEAR..=MAX_INVOICE_YEAR).contains(&y) || !(1..=12).contains(&m) {
        return false;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days_in_month = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if leap {
                29
            } else {
                28
            }
        }
    };
    (1..=days_in_month).contains(&d)
}

/// Normalizes an Indian date string (`DD/MM/YYYY`, `DD-MM-YYYY`, `YYYY-MM-DD`)
/// to ISO-8601 `YYYY-MM-DD`. Impossible dates (31 February, month 13, ...) yield `None`.
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
        if is_valid_calendar_date(y, m, d) {
            return Some(format!("{y:04}-{m:02}-{d:02}"));
        }
    }

    // DD-MM-YYYY or DD/MM/YYYY
    if (1..=2).contains(&parts[0].len()) && (1..=2).contains(&parts[1].len()) && parts[2].len() == 4
    {
        let d: u32 = parts[0].parse().ok()?;
        let m: u32 = parts[1].parse().ok()?;
        let y: u32 = parts[2].parse().ok()?;
        if is_valid_calendar_date(y, m, d) {
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
        if let Some(item) = parse_item_line(token.text.trim(), token.confidence) {
            items.push(item);
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

/// Most tokens accepted from one engine run. A busy invoice is a few hundred.
const MAX_ENGINE_TOKENS: usize = 5_000;
/// Longest text kept for a single token; longer text is cut, not rejected.
const MAX_TOKEN_CHARS: usize = 500;

/// Normalises untrusted engine output before it is parsed or stored: bounded token count,
/// bounded text, no control characters, finite confidence in `0.0..=1.0`, sane boxes.
fn sanitize_tokens(raw: Vec<OcrToken>) -> Result<Vec<OcrToken>, ApiError> {
    if raw.len() > MAX_ENGINE_TOKENS {
        tracing::warn!(tokens = raw.len(), "OCR engine returned too many tokens");
        return Err(ApiError::BadRequest(
            "The OCR engine returned more text than expected. You can enter the bill details manually."
                .into(),
        ));
    }
    Ok(raw
        .into_iter()
        .map(|t| OcrToken {
            text: t
                .text
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .take(MAX_TOKEN_CHARS)
                .collect(),
            confidence: if t.confidence.is_finite() {
                t.confidence.clamp(0.0, 1.0)
            } else {
                0.0
            },
            box_points: t
                .box_points
                .filter(|b| b.iter().flatten().all(|v| v.is_finite())),
        })
        .collect())
}

fn truncate_for_log(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// How OCR should run, derived from the server configuration.
#[derive(Debug, Clone)]
pub struct OcrSettings {
    /// Executable run as `<cmd> <image-path>` (`ARTHA_PADDLE_OCR_CMD`).
    pub cmd: Option<String>,
    pub environment: Environment,
    /// Raw `ARTHA_OCR_USE_DEV_FIXTURES` flag; only honoured in development.
    pub use_dev_fixtures: bool,
    pub timeout: Duration,
    /// Limits how many engine processes may run at once (`ARTHA_OCR_MAX_CONCURRENCY`).
    pub permits: Arc<Semaphore>,
}

impl OcrSettings {
    /// Fixtures are served only when explicitly requested *and* the typed
    /// environment is `Development`. Production ignores the flag.
    pub fn dev_fixtures_allowed(&self) -> bool {
        self.environment.is_development() && self.use_dev_fixtures
    }
}

/// Executes OCR processing on an image file.
///
/// Priority & Subprocess Management:
/// 1. If an external OCR command is configured, it runs through [`runner::run_engine`]: at
///    most `settings.permits` engines at once, one wall-clock budget (`settings.timeout`),
///    bounded stdout/stderr, and the whole process tree (not just the direct child) killed
///    on timeout, oversize output, or request cancellation.
/// 2. If no command is configured, the request fails with an actionable error asking the
///    user to enter the bill by hand, **unless** canned fixtures were explicitly enabled
///    (`ARTHA_OCR_USE_DEV_FIXTURES`) in a development environment. Fixtures are never
///    served by default and never in production.
pub async fn execute_ocr(
    image_path: &Path,
    settings: &OcrSettings,
) -> Result<(OcrResultPayload, ParsedBill), ApiError> {
    let cmd_opt = settings.cmd.as_deref();
    let timeout_duration = settings.timeout;
    if !image_path.exists() {
        return Err(ApiError::NotFound("Image file not found on disk".into()));
    }

    if let Some(cmd) = cmd_opt {
        // Bound how many engine processes run at once. The permit is released on every
        // exit path (success, error, timeout, a dropped request, panic) when it drops.
        // Waiting for a free slot shares the request's time budget.
        let _permit = match tokio::time::timeout(
            timeout_duration,
            settings.permits.clone().acquire_owned(),
        )
        .await
        {
            Ok(Ok(permit)) => permit,
            Ok(Err(_closed)) => {
                return Err(ApiError::Internal("OCR permit pool closed".into()));
            }
            Err(_) => {
                return Err(ApiError::ServiceUnavailable(
                    "The OCR engine is busy with other bills. Wait a moment and try again, or enter the bill details manually."
                        .into(),
                ));
            }
        };

        let output = match runner::run_engine(cmd, image_path, timeout_duration).await {
            Ok(out) => out,
            Err(runner::RunError::Spawn(e)) => {
                // Detail goes to the server log only: the command path and OS error
                // would otherwise reveal the server's filesystem layout to API clients.
                tracing::error!(command = %cmd, error = %e, "failed to start the configured OCR engine");
                return Err(ApiError::BadRequest(
                    "Failed to start the configured OCR engine. Check that ARTHA_PADDLE_OCR_CMD points to an executable file, or enter the bill details manually."
                        .into(),
                ));
            }
            Err(runner::RunError::Io(e)) => {
                return Err(ApiError::Internal(format!("OCR process I/O error: {e}")));
            }
            Err(runner::RunError::TimedOut) => {
                tracing::warn!(
                    timeout_secs = timeout_duration.as_secs(),
                    "OCR engine timed out; its process tree was terminated"
                );
                return Err(ApiError::BadRequest(format!(
                    "OCR processing timed out after {} seconds. You can enter the bill details manually.",
                    timeout_duration.as_secs()
                )));
            }
            Err(runner::RunError::OutputTooLarge) => {
                tracing::warn!(
                    limit_bytes = runner::MAX_STDOUT_BYTES,
                    "OCR engine produced too much output; its process tree was terminated"
                );
                return Err(ApiError::BadRequest(
                    "The OCR engine returned more output than expected. You can enter the bill details manually."
                        .into(),
                ));
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

        let raw_tokens = sanitize_tokens(raw_tokens)?;
        let parsed = parse_bill_tokens(&raw_tokens);
        let payload = OcrResultPayload {
            engine_version: "paddleocr_cli_v1".to_string(),
            confidence: parsed.average_confidence,
            tokens: raw_tokens,
        };
        return Ok((payload, parsed));
    }

    // No OCR command configured: only an explicit development opt-in may serve
    // canned data. Anything else is an error that points the user at manual entry.
    if !settings.dev_fixtures_allowed() {
        return Err(ApiError::BadRequest(
            "Local OCR engine is not configured on this server (ARTHA_PADDLE_OCR_CMD is not set), so this bill cannot be read automatically. Enter the bill details manually, or ask the owner to configure an OCR engine.".into(),
        ));
    }

    // Explicit development opt-in: canned sample data, clearly identified.
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
    fn impossible_calendar_dates_are_not_extracted() {
        assert_eq!(normalize_date("31/02/2026"), None);
        assert_eq!(normalize_date("30/02/2024"), None);
        assert_eq!(normalize_date("29/02/2026"), None, "2026 is not a leap year");
        assert_eq!(normalize_date("29/02/2028"), Some("2028-02-29".to_string()));
        assert_eq!(normalize_date("31/04/2026"), None);
        assert_eq!(normalize_date("00/10/2026"), None);
        assert_eq!(normalize_date("12/13/2026"), None);
        assert_eq!(normalize_date("12/10/9026"), None, "implausible year");
        assert_eq!(normalize_date("12/10/1899"), None, "implausible year");
        assert_eq!(normalize_date("2026-02-30"), None);
        assert_eq!(normalize_date("29/02/1900"), None, "outside the accepted range");
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

        // "2kg" / "1L" are pack sizes, not billed quantities: one pack is suggested but
        // flagged for review instead of being read as "2 units".
        assert_eq!(parsed.items[0].product_name_snapshot, "Organic Sugar");
        assert_eq!(parsed.items[0].quantity_milli, 1000);
        assert!(!parsed.items[0].quantity_certain);
        assert_eq!(parsed.items[0].pack_size.as_deref(), Some("2kg"));
        assert_eq!(parsed.items[0].price_paise, 9000);
        assert_eq!(parsed.items[0].gst_rate_bps, 500);

        assert_eq!(parsed.items[1].product_name_snapshot, "Pure Ghee");
        assert_eq!(parsed.items[1].quantity_milli, 1000);
        assert!(!parsed.items[1].quantity_certain);
        assert_eq!(parsed.items[1].pack_size.as_deref(), Some("1L"));
        assert_eq!(parsed.items[1].price_paise, 55000);
        assert_eq!(parsed.items[1].gst_rate_bps, 1200);
    }

    // ---- quantity versus pack size ----------------------------------------------------

    fn item_for(line: &str) -> Option<ParsedBillItem> {
        parse_item_line(line, 0.9)
    }

    #[test]
    fn pack_sizes_are_not_read_as_quantities() {
        for (line, name, pack) in [
            ("Basmati Rice 5kg 350.00 5%", "Basmati Rice", "5kg"),
            ("Sunflower Oil 1L 140.00 5%", "Sunflower Oil", "1L"),
            ("Wheat Flour 10kg 420.00 0%", "Wheat Flour", "10kg"),
            ("Milk 500ml 30.00", "Milk", "500ml"),
            ("Basmati Rice 5 kg 350.00 5%", "Basmati Rice", "5kg"),
            ("Jeera 250g 80.00", "Jeera", "250g"),
            ("Jeera 1.5kg 80.00", "Jeera", "1.5kg"),
        ] {
            let it = item_for(line).unwrap_or_else(|| panic!("{line:?} should parse"));
            assert_eq!(it.product_name_snapshot, name, "{line}");
            assert_eq!(it.quantity_milli, 1000, "{line}: placeholder is one pack");
            assert!(!it.quantity_certain, "{line}: must require review");
            assert_eq!(it.pack_size.as_deref(), Some(pack), "{line}");
            assert!(it.quantity_note.as_deref().unwrap().contains("pack size"), "{line}");
        }
    }

    #[test]
    fn explicit_quantities_are_certain() {
        for (line, name, milli, price) in [
            ("Soap 3 350.00 5%", "Soap", 3000, 35000),
            ("Soap 3 350.00", "Soap", 3000, 35000),
            ("Sugar Qty 3 90.00", "Sugar", 3000, 9000),
            ("Sugar Qty: 3 90.00", "Sugar", 3000, 9000),
            ("Sugar Qty:3 90.00", "Sugar", 3000, 9000),
            ("Pen 12 pcs 10.00", "Pen", 12000, 1000),
            ("Pen 12pcs 10.00", "Pen", 12000, 1000),
            ("Pen 12 Nos. 10.00", "Pen", 12000, 1000),
            ("Rice 5kg 2 350.00", "Rice 5kg", 2000, 35000),
            ("Onion 2.5 40.00", "Onion", 2500, 4000),
        ] {
            let it = item_for(line).unwrap_or_else(|| panic!("{line:?} should parse"));
            assert_eq!(it.product_name_snapshot, name, "{line}");
            assert_eq!(it.quantity_milli, milli, "{line}");
            assert_eq!(it.price_paise, price, "{line}");
            assert!(it.quantity_certain, "{line}");
            assert!(it.pack_size.is_none() && it.quantity_note.is_none(), "{line}");
        }
    }

    #[test]
    fn multiplier_forms_keep_the_count_but_need_review() {
        for line in ["Sugar 2 x 1kg 90.00", "Sugar 2 X 1kg 90.00", "Sugar 2x1kg 90.00"] {
            let it = item_for(line).unwrap_or_else(|| panic!("{line:?} should parse"));
            assert_eq!(it.product_name_snapshot, "Sugar", "{line}");
            assert_eq!(it.quantity_milli, 2000, "{line}");
            assert!(!it.quantity_certain, "{line}: could be 2 packs or 1 pack of 2");
            assert_eq!(it.pack_size.as_deref(), Some("1kg"), "{line}");
        }
    }

    #[test]
    fn amount_like_numbers_in_the_quantity_slot_are_not_trusted() {
        // rate and line total both present: `100.00` is the rate, not a quantity.
        let it = item_for("Soap 3 100.00 300.00").unwrap();
        assert!(!it.quantity_certain);
        assert!(it.quantity_note.as_deref().unwrap().contains("amount"));
    }

    #[test]
    fn lines_without_a_readable_quantity_or_name_are_skipped_not_guessed() {
        assert!(item_for("3 350.00 5%").is_none(), "no product name");
        assert!(item_for("Rice Flour 90.00").is_none(), "no quantity");
        assert!(item_for("Grand Total: 910.00").is_none());
        assert!(item_for("Total Qty 5 910.00").is_none(), "summary line");
        assert!(item_for("Round Off 1 0.40").is_none(), "summary line");
        assert!(item_for("Rice 0 350.00").is_none(), "zero quantity");
        assert!(item_for("Rice 5 0.00").is_none(), "zero price");
        assert!(item_for("Rice 10A 350.00").is_none(), "unknown unit");
        assert!(item_for("Rice 2.5555 350.00").is_none(), "unrepresentable decimals");
    }

    #[test]
    fn hostile_quantities_never_overflow_or_become_floats() {
        for line in [
            "Rice 99999999999999999999 350.00",
            "Rice 1000001 350.00",
            "Rice 9999999999 350.00",
            "Rice 1e9 350.00",
            "Rice -3 350.00",
            "Rice 3 99999999999999999.99",
            "Rice 5 999999999999.00",
        ] {
            assert!(item_for(line).is_none(), "{line:?} must be rejected");
        }
        // The largest accepted values still produce exact integer milli-units.
        let it = item_for("Rice 1000000 1.00").unwrap();
        assert_eq!(it.quantity_milli, 1_000_000_000);
        let it = item_for("Rice 0.001 1.00").unwrap();
        assert_eq!(it.quantity_milli, 1);
    }

    #[test]
    fn fixture_bill_marks_every_pack_line_for_review() {
        let parsed = parse_bill_tokens(&fixture_tokens());
        assert_eq!(parsed.items.len(), 3);
        assert!(parsed.items.iter().all(|i| !i.quantity_certain));
        assert!(parsed.items.iter().all(|i| i.quantity_milli == 1000));
    }

    fn settings(
        cmd: Option<&str>,
        environment: Environment,
        use_dev_fixtures: bool,
        timeout: Duration,
    ) -> OcrSettings {
        settings_with_permits(
            cmd,
            environment,
            use_dev_fixtures,
            timeout,
            Arc::new(Semaphore::new(2)),
        )
    }

    fn settings_with_permits(
        cmd: Option<&str>,
        environment: Environment,
        use_dev_fixtures: bool,
        timeout: Duration,
        permits: Arc<Semaphore>,
    ) -> OcrSettings {
        OcrSettings {
            cmd: cmd.map(str::to_string),
            environment,
            use_dev_fixtures,
            timeout,
            permits,
        }
    }

    fn temp_image(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("artha_ocr_{tag}_{}.png", std::process::id()));
        std::fs::write(&p, b"test-image-content").unwrap();
        p
    }

    /// Every environment x fixture-flag combination with no OCR engine configured.
    /// Only (development, flag on) may return data, and that data is labelled.
    #[tokio::test]
    async fn fixtures_are_served_only_for_development_with_the_explicit_flag() {
        let image = temp_image("matrix");
        for (env, flag) in [
            (Environment::Development, false),
            (Environment::Production, false),
            (Environment::Production, true),
            (Environment::Development, true),
        ] {
            let result =
                execute_ocr(&image, &settings(None, env, flag, Duration::from_secs(5))).await;
            if env == Environment::Development && flag {
                let (payload, parsed) = result.expect("explicit dev opt-in serves fixtures");
                assert_eq!(payload.engine_version, DEV_FIXTURE_ENGINE_VERSION);
                assert_eq!(parsed.invoice_no, Some("INV-2026-089".to_string()));
                assert_eq!(parsed.items.len(), 3);
            } else {
                match result {
                    Err(ApiError::BadRequest(msg)) => {
                        assert!(
                            msg.contains("Local OCR engine is not configured on this server"),
                            "{env:?}/{flag}: {msg}"
                        );
                        assert!(msg.contains("manually"), "{env:?}/{flag}: {msg}");
                    }
                    other => panic!("{env:?}/{flag}: expected BadRequest, got {other:?}"),
                }
            }
        }
        let _ = std::fs::remove_file(&image);
    }

    #[tokio::test]
    async fn a_configured_engine_is_used_even_when_the_fixture_flag_is_on() {
        // The flag only decides what happens when there is no engine. With an engine
        // configured, a failure must surface as an error, never as fixture data.
        let image = temp_image("engine_over_flag");
        let result = execute_ocr(
            &image,
            &settings(
                Some("/definitely/missing/dir/ocr-engine"),
                Environment::Development,
                true,
                Duration::from_secs(5),
            ),
        )
        .await;
        let _ = std::fs::remove_file(&image);
        assert!(
            matches!(result, Err(ApiError::BadRequest(ref m)) if m.contains("Failed to start")),
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn test_execute_ocr_missing_file_returns_not_found() {
        let nonexistent = Path::new("nonexistent_artha_test_image.png");
        let result = execute_ocr(
            nonexistent,
            &settings(
                None,
                Environment::Development,
                true,
                Duration::from_secs(30),
            ),
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
            &settings(
                Some(&cmd),
                Environment::Development,
                false,
                Duration::from_millis(150),
            ),
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
            &settings(
                Some(&script.to_string_lossy()),
                Environment::Development,
                false,
                Duration::from_secs(20),
            ),
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
            &settings(
                Some("/definitely/missing/dir/ocr-engine"),
                Environment::Development,
                false,
                Duration::from_secs(5),
            ),
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

    #[test]
    fn engine_tokens_are_bounded_and_normalised() {
        let long = "a".repeat(MAX_TOKEN_CHARS * 3);
        let out = sanitize_tokens(vec![
            OcrToken {
                text: "Bill\u{0}No:\u{7}\n1".to_string(),
                confidence: 7.5,
                box_points: None,
            },
            OcrToken {
                text: long,
                confidence: -2.0,
                box_points: Some([[0.0, 0.0], [f32::INFINITY, 0.0], [1.0, 1.0], [0.0, 1.0]]),
            },
            OcrToken {
                text: "x".to_string(),
                confidence: f32::NAN,
                box_points: Some([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]),
            },
        ])
        .unwrap();
        assert_eq!(out[0].text, "Bill No:  1");
        assert_eq!(out[0].confidence, 1.0);
        assert_eq!(out[1].text.chars().count(), MAX_TOKEN_CHARS);
        assert_eq!(out[1].confidence, 0.0);
        assert!(out[1].box_points.is_none(), "non-finite box dropped");
        assert_eq!(out[2].confidence, 0.0);
        assert!(out[2].box_points.is_some());

        let too_many = vec![tok("x"); MAX_ENGINE_TOKENS + 1];
        assert!(matches!(
            sanitize_tokens(too_many),
            Err(ApiError::BadRequest(ref m)) if m.contains("more text than expected")
        ));
    }

    #[tokio::test]
    async fn engine_output_with_invalid_utf8_or_wrong_shape_is_rejected_generically() {
        // 0xFF 0xFE is not valid UTF-8 inside a JSON string.
        let bad_utf8 = script_path(
            "utf8",
            "@echo off\r\necho [{\"text\":\"\u{fffd}\"}]\r\n",
            "#!/bin/sh\nprintf '[{\"text\":\"\\377\\376\",\"confidence\":0.9}]'\n",
        );
        let permits = Arc::new(Semaphore::new(1));
        // On Unix the bytes are genuinely invalid; on Windows the shape is wrong (no
        // confidence). Either way the result is a generic, safe rejection.
        match run_with(&bad_utf8, permits.clone(), Duration::from_secs(20)).await {
            Err(ApiError::BadRequest(msg)) => {
                assert!(msg.contains("could not be read"), "{msg}");
                assert!(!msg.contains("utf"), "parser internals leaked: {msg}");
            }
            other => panic!("Expected a generic BadRequest, got: {other:?}"),
        }
        let _ = std::fs::remove_file(&bad_utf8);
        assert_eq!(permits.available_permits(), 1);
    }

    // ---- concurrency, permit release, and output bounds -------------------------------

    fn script_path(tag: &str, windows_body: &str, unix_body: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        // `write_script` names files by tag + pid, so make the tag unique per call.
        write_script(&format!("{tag}{n}"), windows_body, unix_body)
    }

    async fn run_with(
        script: &Path,
        permits: Arc<Semaphore>,
        timeout: Duration,
    ) -> Result<(OcrResultPayload, ParsedBill), ApiError> {
        let image = temp_image(&format!(
            "perm{}",
            script.file_stem().unwrap().to_string_lossy()
        ));
        let result = execute_ocr(
            &image,
            &settings_with_permits(
                Some(&script.to_string_lossy()),
                Environment::Development,
                false,
                timeout,
                permits,
            ),
        )
        .await;
        let _ = std::fs::remove_file(&image);
        result
    }

    #[tokio::test]
    async fn concurrent_requests_never_run_more_engines_than_the_limit() {
        let log = std::env::temp_dir().join(format!("artha_ocr_conc_{}.log", std::process::id()));
        let _ = std::fs::remove_file(&log);
        let l = log.display();
        let script = script_path(
            "conc",
            &format!(
                "@echo off\r\necho start>>\"{l}\"\r\nping -n 2 127.0.0.1 >nul\r\necho end>>\"{l}\"\r\necho []\r\n"
            ),
            &format!("#!/bin/sh\necho start >> '{l}'\nsleep 0.3\necho end >> '{l}'\necho '[]'\n"),
        );
        let permits = Arc::new(Semaphore::new(1));
        let timeout = Duration::from_secs(60);
        let (a, b, c) = tokio::join!(
            run_with(&script, permits.clone(), timeout),
            run_with(&script, permits.clone(), timeout),
            run_with(&script, permits.clone(), timeout),
        );
        a.unwrap();
        b.unwrap();
        c.unwrap();

        // With one permit the runs are strictly sequential: start,end,start,end,...
        let lines: Vec<String> = std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(|s| s.trim().to_string())
            .collect();
        assert_eq!(
            lines,
            ["start", "end", "start", "end", "start", "end"],
            "engines overlapped despite a limit of 1"
        );
        assert_eq!(permits.available_permits(), 1);
        let _ = std::fs::remove_file(&log);
        let _ = std::fs::remove_file(&script);
    }

    #[tokio::test]
    async fn a_request_waiting_for_a_busy_slot_gets_a_retryable_error() {
        let hang = script_path(
            "busy",
            "@echo off\r\nping -n 60 127.0.0.1 >nul\r\n",
            "#!/bin/sh\nsleep 30\n",
        );
        let permits = Arc::new(Semaphore::new(1));

        let holder = {
            let (hang, permits) = (hang.clone(), permits.clone());
            tokio::spawn(async move { run_with(&hang, permits, Duration::from_secs(4)).await })
        };
        for _ in 0..100 {
            if permits.available_permits() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(
            permits.available_permits(),
            0,
            "the first engine should hold the slot"
        );

        let second = run_with(&hang, permits.clone(), Duration::from_millis(300)).await;
        assert!(
            matches!(second, Err(ApiError::ServiceUnavailable(ref m)) if m.contains("busy")),
            "{second:?}"
        );

        // The holder times out, which must free its slot again.
        let first = holder.await.unwrap();
        assert!(
            matches!(first, Err(ApiError::BadRequest(ref m)) if m.contains("timed out")),
            "{first:?}"
        );
        assert_eq!(permits.available_permits(), 1);
        let _ = std::fs::remove_file(&hang);
    }

    #[tokio::test]
    async fn the_slot_is_released_after_every_kind_of_outcome() {
        let permits = Arc::new(Semaphore::new(1));
        let ok = script_path("rel_ok", "@echo []\r\n", "#!/bin/sh\necho '[]'\n");
        let exit3 = script_path("rel_exit", "@exit /b 3\r\n", "#!/bin/sh\nexit 3\n");
        let junk = script_path(
            "rel_junk",
            "@echo not-json\r\n",
            "#!/bin/sh\necho not-json\n",
        );
        let hang = script_path(
            "rel_hang",
            "@echo off\r\nping -n 60 127.0.0.1 >nul\r\n",
            "#!/bin/sh\nsleep 30\n",
        );

        assert!(run_with(&ok, permits.clone(), Duration::from_secs(20))
            .await
            .is_ok());
        assert_eq!(permits.available_permits(), 1, "after success");
        assert!(run_with(&exit3, permits.clone(), Duration::from_secs(20))
            .await
            .is_err());
        assert_eq!(permits.available_permits(), 1, "after a non-zero exit");
        assert!(run_with(&junk, permits.clone(), Duration::from_secs(20))
            .await
            .is_err());
        assert_eq!(permits.available_permits(), 1, "after unreadable output");
        assert!(run_with(&hang, permits.clone(), Duration::from_millis(300))
            .await
            .is_err());
        assert_eq!(permits.available_permits(), 1, "after a timeout");

        for p in [ok, exit3, junk, hang] {
            let _ = std::fs::remove_file(p);
        }
    }

    #[tokio::test]
    async fn the_slot_is_released_when_the_request_is_dropped() {
        let hang = script_path(
            "drop",
            "@echo off\r\nping -n 60 127.0.0.1 >nul\r\n",
            "#!/bin/sh\nsleep 30\n",
        );
        let permits = Arc::new(Semaphore::new(1));
        let task = {
            let (hang, permits) = (hang.clone(), permits.clone());
            tokio::spawn(async move { run_with(&hang, permits, Duration::from_secs(120)).await })
        };
        for _ in 0..100 {
            if permits.available_permits() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(permits.available_permits(), 0);

        task.abort();
        let _ = task.await;
        assert_eq!(
            permits.available_permits(),
            1,
            "a dropped request must free its slot"
        );
        let _ = std::fs::remove_file(&hang);
    }

    #[tokio::test]
    async fn runaway_engine_output_gets_a_generic_error_and_frees_the_slot() {
        let permits = Arc::new(Semaphore::new(1));
        let flood = script_path(
            "flood",
            "@echo off\r\nfor /l %%i in (1,1,200000) do @echo xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\r\nping -n 60 127.0.0.1 >nul\r\n",
            "#!/bin/sh\nyes | head -c 8000000\nsleep 30\n",
        );
        let result = run_with(&flood, permits.clone(), Duration::from_secs(60)).await;
        match result {
            Err(ApiError::BadRequest(msg)) => {
                assert!(msg.contains("more output than expected"), "{msg}");
                assert!(
                    !msg.contains(&*flood.to_string_lossy()),
                    "path leaked: {msg}"
                );
            }
            other => panic!("Expected ApiError::BadRequest, got: {other:?}"),
        }
        assert_eq!(permits.available_permits(), 1);
        let _ = std::fs::remove_file(&flood);
    }
}
