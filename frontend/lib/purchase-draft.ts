// Pure helpers for the purchase draft review screen (Phase 10). Kept free of React and
// network code so the rules that protect users from wrong OCR data can be unit-tested.

import type { ParsedOcrBill, ProductView } from "./api";

export interface DraftItem {
  purchase_item_id?: string;
  product_id?: string;
  product_name_snapshot?: string;
  quantity_milli: number;
  price_paise: number;
  gst_rate_bps: number;
  /** OCR reading confidence (0..1). Client-only; not persisted. */
  confidence?: number;
  /**
   * Set while OCR could not tell the billed quantity from a pack size. The line cannot be
   * approved until a person confirms (or changes) the quantity. Client-only.
   */
  qtyReview?: { note: string; pack?: string } | null;
}

const MIN_YEAR = 1990;
const MAX_YEAR = 2100;

/** True for a real `YYYY-MM-DD` calendar date. Mirrors the server's rule exactly. */
export function isValidIsoDate(value: string): boolean {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!m) return false;
  const y = Number(m[1]);
  const mo = Number(m[2]);
  const d = Number(m[3]);
  if (y < MIN_YEAR || y > MAX_YEAR || mo < 1 || mo > 12) return false;
  const leap = (y % 4 === 0 && y % 100 !== 0) || y % 400 === 0;
  const dim = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][mo - 1];
  return d >= 1 && d <= dim;
}

export interface HeaderState {
  invoiceNo: string;
  invoiceDate: string;
  /** An OCR date that differs from what the user already has; offered, never applied. */
  dateSuggestion: string | null;
}

/**
 * Applies OCR header suggestions without overwriting the user's work.
 * - Invoice number: filled from OCR when OCR found one (existing behaviour).
 * - Invoice date: filled only when the field is empty. A date that is already present
 *   (loaded from the server or typed by the user) is kept, and a different OCR date is
 *   only offered as `dateSuggestion`.
 */
export function applyOcrToHeader(
  header: HeaderState,
  parsed: Pick<ParsedOcrBill, "invoice_no" | "invoice_date">
): HeaderState {
  const next: HeaderState = { ...header, dateSuggestion: null };
  if (parsed.invoice_no) next.invoiceNo = parsed.invoice_no;

  const ocrDate = parsed.invoice_date;
  if (ocrDate && isValidIsoDate(ocrDate)) {
    if (header.invoiceDate.trim() === "") {
      next.invoiceDate = ocrDate;
    } else if (header.invoiceDate !== ocrDate) {
      next.dateSuggestion = ocrDate;
    }
  }
  return next;
}

/** Turns parsed OCR lines into editable draft lines, matching catalog products by exact name. */
export function mapOcrItems(parsed: Pick<ParsedOcrBill, "items">, products: ProductView[]): DraftItem[] {
  return (parsed.items || []).map((i) => {
    const name = String(i.product_name_snapshot || "");
    const matched = products.find((p) => p.name.trim().toLowerCase() === name.trim().toLowerCase());
    // Only an explicit `true` means the quantity was actually read; a missing flag (older
    // server) is treated as unverified rather than trusted.
    const certain = i.quantity_certain === true;
    return {
      product_id: matched ? matched.product_id : "",
      product_name_snapshot: name,
      quantity_milli: Number(i.quantity_milli || 1000),
      price_paise: Number(i.price_paise || 0),
      gst_rate_bps: Number(i.gst_rate_bps || 0),
      confidence: i.confidence,
      qtyReview: certain
        ? null
        : {
            note:
              i.quantity_note ||
              "The billed quantity could not be read reliably. Check the bill and confirm the quantity.",
            pack: i.pack_size,
          },
    };
  });
}

/**
 * After a save the server returns the authoritative lines. Carry over the client-only
 * annotations (confidence, pending quantity review) by position so saving a draft never
 * silently drops a "confirm this quantity" requirement. Falls back to the server lines
 * unchanged if the shape no longer lines up.
 */
export function mergeSavedItems(
  previous: DraftItem[],
  saved: Array<Omit<DraftItem, "qtyReview" | "confidence">>
): DraftItem[] {
  if (previous.length !== saved.length) return saved.map((s) => ({ ...s }));
  return saved.map((s, idx) => {
    const p = previous[idx];
    return {
      ...s,
      confidence: p.confidence,
      // The review is about the quantity, so it survives unless the quantity changed.
      qtyReview: p.quantity_milli === s.quantity_milli ? (p.qtyReview ?? null) : null,
    };
  });
}

/** Marks a line's quantity as reviewed (user confirmed it, or edited it). */
export function confirmQuantity(items: DraftItem[], index: number): DraftItem[] {
  return items.map((it, i) => (i === index ? { ...it, qtyReview: null } : it));
}

/** Editing a quantity counts as the human review that the line was waiting for. */
export function setQuantity(items: DraftItem[], index: number, quantityMilli: number): DraftItem[] {
  return items.map((it, i) =>
    i === index ? { ...it, quantity_milli: quantityMilli, qtyReview: null } : it
  );
}

export interface ApprovalCheck {
  ok: boolean;
  reason: string | null;
}

/** Whether the draft may be approved from the client's point of view. */
export function checkApprovable(items: DraftItem[], invoiceDate: string): ApprovalCheck {
  if (items.some((i) => !i.product_id)) {
    return { ok: false, reason: "Match every line to a catalog product." };
  }
  if (items.some((i) => i.qtyReview)) {
    return { ok: false, reason: "Confirm the quantity on every highlighted line." };
  }
  if (invoiceDate.trim() !== "" && !isValidIsoDate(invoiceDate.trim())) {
    return { ok: false, reason: "Enter the invoice date as a valid date." };
  }
  return { ok: true, reason: null };
}

/** The date to send on save: a valid ISO date, or `null` to clear it. */
export function invoiceDateForSave(value: string): { ok: true; value: string | null } | { ok: false; error: string } {
  const v = value.trim();
  if (v === "") return { ok: true, value: null };
  if (!isValidIsoDate(v)) return { ok: false, error: "Invoice date is not a valid date." };
  return { ok: true, value: v };
}

export interface SavePayload {
  invoice_no: string;
  invoice_date: string | null;
  items: Array<{
    purchase_item_id?: string;
    product_id: string | null;
    product_name_snapshot?: string;
    quantity_milli: number;
    price_paise: number;
    gst_rate_bps: number;
  }>;
}

/** Builds the PATCH body for saving a draft; the date is always sent (value or null). */
export function buildSavePayload(
  invoiceNo: string,
  invoiceDate: string,
  items: DraftItem[]
): { ok: true; payload: SavePayload } | { ok: false; error: string } {
  const date = invoiceDateForSave(invoiceDate);
  if (!date.ok) return date;
  return {
    ok: true,
    payload: {
      invoice_no: invoiceNo,
      invoice_date: date.value,
      items: items.map((it) => ({
        purchase_item_id: it.purchase_item_id,
        product_id: it.product_id ? it.product_id : null,
        product_name_snapshot: it.product_name_snapshot,
        quantity_milli: it.quantity_milli,
        price_paise: it.price_paise,
        gst_rate_bps: it.gst_rate_bps,
      })),
    },
  };
}

/**
 * The sample-data warning is shown if the server says any OCR run on this purchase
 * returned canned data (survives reloads) or the latest response in this session did.
 */
export function fixtureWarningVisible(serverFlag: boolean | undefined, sessionFlag: boolean): boolean {
  return serverFlag === true || sessionFlag;
}
