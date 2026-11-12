import { test, describe, beforeEach, afterEach } from "node:test";
import assert from "node:assert/strict";
import { api } from "../lib/api.ts";
import {
  isValidIsoDate,
  applyOcrToHeader,
  mapOcrItems,
  mergeSavedItems,
  confirmQuantity,
  setQuantity,
  checkApprovable,
  invoiceDateForSave,
  buildSavePayload,
  fixtureWarningVisible,
} from "../lib/purchase-draft.ts";

const products = [{ product_id: "p-rice", name: "Basmati Rice", qr_code: "QR-RICE" }];

const ocrItem = (over = {}) => ({
  product_name_snapshot: "Basmati Rice",
  quantity_milli: 1000,
  quantity_certain: true,
  price_paise: 35000,
  gst_rate_bps: 500,
  confidence: 0.92,
  ...over,
});

describe("isValidIsoDate (mirrors the server rule)", () => {
  test("accepts real dates, including a leap day", () => {
    for (const d of ["2026-10-12", "2028-02-29", "1990-01-01", "2100-12-31"]) {
      assert.equal(isValidIsoDate(d), true, d);
    }
  });
  test("rejects impossible, malformed and implausible dates", () => {
    for (const d of [
      "2026-02-30", "2025-02-29", "2026-13-01", "2026-00-10", "2026-04-31",
      "12/10/2026", "2026-1-5", "2026-10-12T00:00:00", "", "not-a-date", "9999-01-01", "1899-12-31",
    ]) {
      assert.equal(isValidIsoDate(d), false, d);
    }
  });
});

describe("invoice date from OCR to the user's field", () => {
  const header = (over = {}) => ({ invoiceNo: "", invoiceDate: "", dateSuggestion: null, ...over });

  test("fills an empty date from OCR", () => {
    const h = applyOcrToHeader(header(), { invoice_no: "INV-1", invoice_date: "2026-10-12" });
    assert.equal(h.invoiceDate, "2026-10-12");
    assert.equal(h.invoiceNo, "INV-1");
    assert.equal(h.dateSuggestion, null);
  });

  test("never overwrites a date that is already there; offers the OCR date instead", () => {
    const h = applyOcrToHeader(header({ invoiceDate: "2026-10-11" }), { invoice_date: "2026-10-12" });
    assert.equal(h.invoiceDate, "2026-10-11", "manual/stored date must survive a re-scan");
    assert.equal(h.dateSuggestion, "2026-10-12");
  });

  test("a repeated OCR run with the same date raises no suggestion", () => {
    const h = applyOcrToHeader(header({ invoiceDate: "2026-10-12" }), { invoice_date: "2026-10-12" });
    assert.equal(h.invoiceDate, "2026-10-12");
    assert.equal(h.dateSuggestion, null);
  });

  test("an OCR date that is not a real date is ignored", () => {
    const h = applyOcrToHeader(header(), { invoice_date: "2026-02-30" });
    assert.equal(h.invoiceDate, "");
  });

  test("no OCR date leaves manual entry untouched", () => {
    const h = applyOcrToHeader(header({ invoiceDate: "2026-09-01" }), { invoice_no: "X-9" });
    assert.equal(h.invoiceDate, "2026-09-01");
    assert.equal(h.dateSuggestion, null);
  });
});

describe("save payload", () => {
  test("always carries the date (or null) and validates it first", () => {
    const ok = buildSavePayload("A-1", "2026-10-12", []);
    assert.equal(ok.ok, true);
    assert.equal(ok.payload.invoice_date, "2026-10-12");

    const cleared = buildSavePayload("A-1", "  ", []);
    assert.equal(cleared.payload.invoice_date, null);
    assert.ok("invoice_date" in cleared.payload, "null must be sent explicitly, not omitted");

    const bad = buildSavePayload("A-1", "2026-02-30", []);
    assert.equal(bad.ok, false);
    assert.equal(invoiceDateForSave("12/10/2026").ok, false);
  });

  test("unmatched lines are sent with a null product", () => {
    const r = buildSavePayload("", "", [{ product_id: "", quantity_milli: 1000, price_paise: 5, gst_rate_bps: 0 }]);
    assert.equal(r.payload.items[0].product_id, null);
  });
});

describe("quantity versus pack size on the review screen", () => {
  test("a certain quantity needs no review; an uncertain or unflagged one does", () => {
    const [certain, pack, legacy] = mapOcrItems(
      {
        items: [
          ocrItem(),
          ocrItem({ quantity_certain: false, pack_size: "5kg", quantity_note: "5kg looks like a pack size" }),
          ocrItem({ quantity_certain: undefined }),
        ],
      },
      products
    );
    assert.equal(certain.qtyReview, null);
    assert.equal(pack.qtyReview.pack, "5kg");
    assert.match(pack.qtyReview.note, /pack size/);
    assert.ok(legacy.qtyReview, "a missing flag must not be trusted");
    assert.equal(certain.product_id, "p-rice", "catalog match by name still works");
  });

  test("approval is blocked until each uncertain quantity is confirmed or edited", () => {
    let items = mapOcrItems({ items: [ocrItem({ quantity_certain: false })] }, products);
    assert.equal(checkApprovable(items, "").ok, false);
    assert.match(checkApprovable(items, "").reason, /quantity/i);

    assert.equal(checkApprovable(confirmQuantity(items, 0), "").ok, true);

    items = setQuantity(items, 0, 2000);
    assert.equal(items[0].quantity_milli, 2000);
    assert.equal(checkApprovable(items, "").ok, true, "editing the quantity is the review");
  });

  test("approval is also blocked by unmatched products and an invalid date", () => {
    const unmatched = [{ product_id: "", quantity_milli: 1000, price_paise: 1, gst_rate_bps: 0 }];
    assert.equal(checkApprovable(unmatched, "").ok, false);
    const matched = [{ product_id: "p", quantity_milli: 1000, price_paise: 1, gst_rate_bps: 0 }];
    assert.equal(checkApprovable(matched, "2026-02-30").ok, false);
    assert.equal(checkApprovable(matched, "2026-10-12").ok, true);
    assert.equal(checkApprovable(matched, "").ok, true, "a blank date is allowed");
  });

  test("a pending review survives saving a draft unless the quantity changed", () => {
    const before = mapOcrItems({ items: [ocrItem({ quantity_certain: false })] }, products);
    const saved = [{ purchase_item_id: "i-1", product_id: "p-rice", product_name_snapshot: "Basmati Rice",
      quantity_milli: 1000, price_paise: 35000, gst_rate_bps: 500 }];
    const after = mergeSavedItems(before, saved);
    assert.ok(after[0].qtyReview, "review must not be dropped by a save");
    assert.equal(after[0].purchase_item_id, "i-1");
    assert.equal(after[0].confidence, 0.92);

    const changed = mergeSavedItems(before, [{ ...saved[0], quantity_milli: 2000 }]);
    assert.equal(changed[0].qtyReview, null);

    const reshaped = mergeSavedItems(before, []);
    assert.deepEqual(reshaped, []);
  });
});

describe("sample-data (fixture) warning", () => {
  test("stays visible if either the server or this session says fixture data was produced", () => {
    assert.equal(fixtureWarningVisible(true, false), true, "after a reload the server flag keeps it");
    assert.equal(fixtureWarningVisible(false, true), true, "within the session");
    assert.equal(fixtureWarningVisible(true, true), true);
    assert.equal(fixtureWarningVisible(false, false), false);
    assert.equal(fixtureWarningVisible(undefined, false), false);
  });
});

describe("invoice date: extract, edit, save, reload (through the real api client)", () => {
  let realFetch;
  let calls;
  let server; // minimal in-memory purchase

  beforeEach(() => {
    realFetch = globalThis.fetch;
    calls = [];
    server = { invoice_no: null, invoice_date: null, items: [] };
    globalThis.window = { localStorage: { getItem: () => "tok", setItem() {}, removeItem() {} } };
    globalThis.fetch = async (url, init = {}) => {
      calls.push({ url: String(url), init });
      if (init.method === "PATCH") {
        const body = JSON.parse(init.body);
        // Server semantics: omitted keeps, null/"" clears, a string sets.
        if ("invoice_date" in body) server.invoice_date = body.invoice_date || null;
        server.invoice_no = body.invoice_no;
        return new Response(JSON.stringify({ ok: true }), { status: 200 });
      }
      return new Response(JSON.stringify({ purchase_id: "p-1", ...server, status: "draft", source: "ocr" }), { status: 200 });
    };
  });

  afterEach(() => {
    globalThis.fetch = realFetch;
    delete globalThis.window;
  });

  test("OCR date -> user edits it -> save -> reload shows the edited date; a re-scan keeps it", async () => {
    // 1. OCR extracts a date into the empty field.
    let header = applyOcrToHeader(
      { invoiceNo: "", invoiceDate: "", dateSuggestion: null },
      { invoice_no: "INV-1", invoice_date: "2026-10-12" }
    );
    assert.equal(header.invoiceDate, "2026-10-12");

    // 2. The user corrects it by hand.
    header = { ...header, invoiceDate: "2026-10-11" };

    // 3. Save: the request carries the edited date.
    const built = buildSavePayload(header.invoiceNo, header.invoiceDate, []);
    assert.equal(built.ok, true);
    await api.updatePurchase("p-1", built.payload);
    const patch = calls.find((c) => c.init.method === "PATCH");
    assert.equal(JSON.parse(patch.init.body).invoice_date, "2026-10-11");

    // 4. Reload: the screen initialises from the server and shows the edited date.
    const reloaded = await api.getPurchase("p-1");
    assert.equal(reloaded.invoice_date, "2026-10-11");
    let after = { invoiceNo: reloaded.invoice_no || "", invoiceDate: reloaded.invoice_date || "", dateSuggestion: null };

    // 5. Re-scanning the same bill must not replace the user's date.
    after = applyOcrToHeader(after, { invoice_no: "INV-1", invoice_date: "2026-10-12" });
    assert.equal(after.invoiceDate, "2026-10-11");
    assert.equal(after.dateSuggestion, "2026-10-12");
  });

  test("a purchase saved before this feature (no date) loads and saves cleanly", async () => {
    const reloaded = await api.getPurchase("p-1");
    assert.equal(reloaded.invoice_date ?? null, null);
    const built = buildSavePayload("", reloaded.invoice_date || "", []);
    assert.equal(built.payload.invoice_date, null);
  });
});

import { describeBackupPhotos, describeRestoredPhotos } from "../lib/api.ts";

describe("backup photo wording", () => {
  test("lists what a backup holds and never hides missing photos", () => {
    assert.equal(describeBackupPhotos({}), "Database only");
    assert.equal(describeBackupPhotos({ images_included: false, image_count: 0 }), "Database only");
    assert.equal(describeBackupPhotos({ images_included: true, image_count: 1, images_missing: 0 }), "1 photo");
    assert.equal(describeBackupPhotos({ images_included: true, image_count: 3, images_missing: 0 }), "3 photos");
    assert.equal(describeBackupPhotos({ images_included: true, image_count: 3, images_missing: 2 }), "3 photos, 2 missing");
  });

  test("restore message mentions restored photos and warns about unavailable ones", () => {
    assert.equal(describeRestoredPhotos(undefined), "");
    assert.equal(describeRestoredPhotos({ included: true, restored: 0, already_present: 4, missing: 0 }), "");
    assert.match(describeRestoredPhotos({ included: true, restored: 2, already_present: 0, missing: 0 }), /2 bill photos restored/);
    const legacy = describeRestoredPhotos({ included: false, restored: 0, already_present: 0, missing: 1 });
    assert.match(legacy, /WARNING: 1 bill photo is not available/);
    assert.match(legacy, /before photos were included/);
  });
});
