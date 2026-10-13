"use client";

import { useEffect, useState, useRef } from "react";
import { useParams, useRouter } from "next/navigation";
import Link from "next/link";
import { api, formatPaise, ProductView, ParsedOcrBill } from "@/lib/api";
import LoadingState from "@/components/ui/LoadingState";
import ErrorState from "@/components/ui/ErrorState";

export default function PurchaseDetailPage() {
  const params = useParams();
  const purchaseId = params.id as string;
  const router = useRouter();

  const [purchase, setPurchase] = useState<{
    purchase_id: string;
    status: string;
    source: string;
    subtotal_paise: number;
    tax_total_paise: number;
    total_paise: number;
    image_id?: string;
  } | null>(null);
  const [products, setProducts] = useState<ProductView[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Draft Edit State
  const [invoiceNo, setInvoiceNo] = useState("");
  const [items, setItems] = useState<
    Array<{
      purchase_item_id?: string;
      product_id?: string;
      product_name_snapshot?: string;
      quantity_milli: number;
      price_paise: number;
      gst_rate_bps: number;
      confidence?: number;
    }>
  >([]);
  const [parsedOcr, setParsedOcr] = useState<ParsedOcrBill | null>(null);
  const [saving, setSaving] = useState(false);
  const [approving, setApproving] = useState(false);
  const [ocrProcessing, setOcrProcessing] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    let mounted = true;
    const fetchInitialData = async () => {
      try {
        setLoading(true);
        const [purchaseData, productsData] = await Promise.all([
          api.getPurchase(purchaseId),
          api.listProducts(),
        ]);
        if (mounted) {
          setPurchase(purchaseData);
          setProducts(productsData);
          setInvoiceNo(purchaseData.invoice_no || "");
          setItems(purchaseData.items || []);
          setError(null);
        }
      } catch (err: unknown) {
        if (mounted) setError(err instanceof Error ? err.message : "Failed to load purchase");
      } finally {
        if (mounted) setLoading(false);
      }
    };
    fetchInitialData();
    return () => {
      mounted = false;
    };
  }, [purchaseId]);

  const handleSaveDraft = async () => {
    setSaving(true);
    setError(null);
    try {
      await api.updatePurchase(purchaseId, {
        invoice_no: invoiceNo,
        items: items.map((it) => ({
          purchase_item_id: it.purchase_item_id,
          product_id: it.product_id ? it.product_id : null,
          product_name_snapshot: it.product_name_snapshot,
          quantity_milli: it.quantity_milli,
          price_paise: it.price_paise,
          gst_rate_bps: it.gst_rate_bps,
        })),
      });
      // Refresh purchase to reflect updated authoritative backend totals
      const data = await api.getPurchase(purchaseId);
      setPurchase(data);
      setItems(data.items || []);
      alert("Draft saved!");
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to save draft");
    } finally {
      setSaving(false);
    }
  };

  const handleApprove = async () => {
    if (!confirm("Are you sure? This will update inventory and cannot be undone.")) return;
    setApproving(true);
    setError(null);
    try {
      await handleSaveDraft(); // Save first to ensure server has current edits
      await api.approvePurchase(purchaseId);
      router.push("/purchases");
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to approve purchase");
    } finally {
      setApproving(false);
    }
  };

  const handleCancel = async () => {
    if (!confirm("Void this draft?")) return;
    try {
      await api.cancelPurchase(purchaseId);
      router.push("/purchases");
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to void");
    }
  };

  const handleFileUpload = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    setOcrProcessing(true);
    setError(null);
    try {
      const { image_id } = await api.uploadBillImage(purchaseId, file);
      const ocrRes = await api.processOcr(purchaseId, image_id);

      if (ocrRes.parsed) {
        setParsedOcr(ocrRes.parsed);
        if (ocrRes.parsed.invoice_no) {
          setInvoiceNo(ocrRes.parsed.invoice_no);
        }
        const ocrItems = (ocrRes.parsed.items || []).map((i) => {
          // Attempt smart match with catalog products by name
          const matched = products.find(
            (p) => p.name.trim().toLowerCase() === i.product_name_snapshot.trim().toLowerCase()
          );
          return {
            product_id: matched ? matched.product_id : "",
            product_name_snapshot: String(i.product_name_snapshot || ""),
            quantity_milli: Number(i.quantity_milli || 1000),
            price_paise: Number(i.price_paise || 0),
            gst_rate_bps: Number(i.gst_rate_bps || 0),
            confidence: i.confidence,
          };
        });
        setItems(ocrItems);
      }

      // Re-fetch purchase data to link image_id in header
      const refreshed = await api.getPurchase(purchaseId);
      setPurchase(refreshed);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "OCR Processing failed");
    } finally {
      setOcrProcessing(false);
      if (fileInputRef.current) fileInputRef.current.value = "";
    }
  };

  if (loading && !purchase) return <LoadingState />;
  if (error && !purchase) return <ErrorState message={error} onRetry={() => window.location.reload()} />;
  if (!purchase) return null;

  const isDraft = purchase.status === "draft";
  const hasUnresolved = items.some((i) => !i.product_id);

  // Client-side tentative totals calculation for draft view
  const tentativeSubtotal = items.reduce(
    (acc, it) => acc + Math.round((it.quantity_milli / 1000) * it.price_paise),
    0
  );
  const tentativeTax = items.reduce((acc, it) => {
    const lineTotal = Math.round((it.quantity_milli / 1000) * it.price_paise);
    return acc + Math.round((lineTotal * it.gst_rate_bps) / 10000);
  }, 0);
  const tentativeGrandTotal = tentativeSubtotal + tentativeTax;

  return (
    <div>
      <div style={{ marginBottom: "2rem" }}>
        <Link href="/purchases" className="btn btn-secondary" style={{ marginBottom: "1rem", display: "inline-block" }}>
          &larr; Back to Purchases
        </Link>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-end" }}>
          <div>
            <h1 style={{ margin: 0 }}>
              {isDraft ? "Review Purchase Draft" : "Purchase Details"}
            </h1>
            <p className="muted" style={{ margin: "0.5rem 0 0", textTransform: "capitalize" }}>
              Status: {purchase.status} | Source: {purchase.source}
            </p>
          </div>
          {isDraft && (
            <div style={{ display: "flex", gap: "1rem" }}>
              <button
                className="btn btn-secondary"
                onClick={handleCancel}
                style={{ borderColor: "var(--danger)", color: "var(--danger)" }}
              >
                Void Draft
              </button>
              <button
                className="btn"
                onClick={handleApprove}
                disabled={approving || saving || hasUnresolved}
              >
                {approving ? "Approving..." : "Approve & Update Inventory"}
              </button>
            </div>
          )}
        </div>
      </div>

      {error && (
        <div
          className="card"
          style={{ marginBottom: "1rem", borderColor: "var(--danger)", backgroundColor: "rgba(179, 38, 30, 0.05)" }}
        >
          <p style={{ color: "var(--danger)", margin: 0 }}>{error}</p>
        </div>
      )}

      {/* Bill Image Upload & OCR Trigger */}
      {isDraft && purchase.source === "ocr" && (
        <div
          className="card"
          style={{
            marginBottom: "2rem",
            textAlign: "center",
            border: "2px dashed var(--accent)",
            backgroundColor: "rgba(15, 107, 76, 0.03)",
          }}
        >
          <h3>📷 {purchase.image_id ? "Replace Bill Image" : "Upload Bill Photo"}</h3>
          <p className="muted">
            {purchase.image_id
              ? "A bill image is attached. You can upload a clearer photo if needed."
              : "Upload an image of the invoice to extract items automatically."}
          </p>
          <input
            type="file"
            accept="image/png,image/jpeg,image/webp"
            onChange={handleFileUpload}
            ref={fileInputRef}
            style={{ display: "none" }}
          />
          <button
            className="btn"
            onClick={() => fileInputRef.current?.click()}
            disabled={ocrProcessing}
          >
            {ocrProcessing ? "Scanning & Extracting with OCR..." : purchase.image_id ? "Re-upload & Scan" : "Select Image & Process OCR"}
          </button>
        </div>
      )}

      {/* Split View: Bill Preview + Invoice Review */}
      <div style={{ display: "grid", gridTemplateColumns: purchase.image_id ? "1fr 1.6fr" : "1fr", gap: "1.5rem", marginBottom: "2rem" }}>
        {purchase.image_id && (
          <div className="card" style={{ display: "flex", flexDirection: "column" }}>
            <h3 style={{ marginTop: 0 }}>📷 Attached Bill Photo</h3>
            <div
              style={{
                flex: 1,
                minHeight: "350px",
                maxHeight: "550px",
                overflow: "auto",
                border: "1px solid var(--border)",
                borderRadius: 6,
                backgroundColor: "#f9f9f9",
                display: "flex",
                justifyContent: "center",
                alignItems: "center",
              }}
            >
              {/* eslint-disable-next-line @next/next/no-img-element */}
              <img
                src={api.getBillImageUrl(purchaseId)}
                alt="Original Bill"
                style={{ maxWidth: "100%", height: "auto", display: "block" }}
              />
            </div>
          </div>
        )}

        <div>
          {/* Metadata */}
          <div className="card" style={{ marginBottom: "1.5rem" }}>
            <h3 style={{ marginTop: 0 }}>Invoice Metadata</h3>
            <div className="field" style={{ maxWidth: 320 }}>
              <label>Invoice Number</label>
              <input
                value={invoiceNo}
                onChange={(e) => setInvoiceNo(e.target.value)}
                disabled={!isDraft}
                placeholder="e.g. INV-2026-089"
              />
              {parsedOcr?.invoice_no_confidence && (
                <span className="muted" style={{ fontSize: "0.75rem", marginTop: "0.2rem", display: "block" }}>
                  Confidence: {Math.round(parsedOcr.invoice_no_confidence * 100)}%
                </span>
              )}
            </div>
          </div>

          {/* Reconciliation Banner */}
          {parsedOcr?.stated_total_paise && (
            <div
              className="card"
              style={{
                marginBottom: "1.5rem",
                border: `1px solid ${parsedOcr.stated_total_paise === tentativeGrandTotal ? "var(--accent)" : "var(--danger)"}`,
                backgroundColor: parsedOcr.stated_total_paise === tentativeGrandTotal ? "rgba(15, 107, 76, 0.05)" : "rgba(179, 38, 30, 0.05)",
              }}
            >
              <h4 style={{ margin: "0 0 0.5rem" }}>Reconciliation Verification</h4>
              <div style={{ display: "flex", justifyContent: "space-between", fontSize: "0.9rem" }}>
                <span>Stated Bill Total: <strong>{formatPaise(parsedOcr.stated_total_paise)}</strong></span>
                <span>Calculated Total: <strong>{formatPaise(tentativeGrandTotal)}</strong></span>
                <span>
                  {parsedOcr.stated_total_paise === tentativeGrandTotal ? (
                    <strong style={{ color: "var(--accent)" }}>✓ Balanced</strong>
                  ) : (
                    <strong style={{ color: "var(--danger)" }}>
                      Difference: {formatPaise(Math.abs(parsedOcr.stated_total_paise - tentativeGrandTotal))}
                    </strong>
                  )}
                </span>
              </div>
            </div>
          )}
        </div>
      </div>

      {/* Line Items Table */}
      <div className="card">
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <h3 style={{ margin: 0 }}>Line Items</h3>
          {isDraft && (
            <button
              className="btn btn-secondary"
              onClick={() =>
                setItems([
                  ...items,
                  {
                    product_id: "",
                    product_name_snapshot: "",
                    quantity_milli: 1000,
                    price_paise: 0,
                    gst_rate_bps: 0,
                  },
                ])
              }
            >
              + Add Item
            </button>
          )}
        </div>

        {hasUnresolved && isDraft && (
          <div
            style={{
              background: "#fef7e0",
              color: "#b06000",
              padding: "0.6rem 0.8rem",
              borderRadius: 4,
              marginTop: "1rem",
              fontSize: "0.9rem",
            }}
          >
            <strong>Action Required:</strong> Unresolved items must be matched to a product in the catalog before approval.
          </div>
        )}

        <table style={{ width: "100%", marginTop: "1rem" }}>
          <thead>
            <tr>
              <th style={{ textAlign: "left" }}>Extracted Name / Catalog Product</th>
              <th style={{ textAlign: "right", width: "110px" }}>Qty (Milli)</th>
              <th style={{ textAlign: "right", width: "120px" }}>Price (Paise)</th>
              <th style={{ textAlign: "right", width: "100px" }}>GST (Bps)</th>
              {isDraft && <th style={{ width: "50px" }}></th>}
            </tr>
          </thead>
          <tbody>
            {items.map((it, idx) => (
              <tr key={idx} style={{ background: !it.product_id ? "rgba(179, 38, 30, 0.05)" : "transparent" }}>
                <td>
                  <input
                    value={it.product_name_snapshot || ""}
                    onChange={(e) => {
                      const newItems = [...items];
                      newItems[idx].product_name_snapshot = e.target.value;
                      setItems(newItems);
                    }}
                    disabled={!isDraft}
                    placeholder="Extracted Name"
                    style={{ width: "100%", marginBottom: "0.4rem" }}
                  />
                  {isDraft ? (
                    <select
                      value={it.product_id || ""}
                      onChange={(e) => {
                        const newItems = [...items];
                        newItems[idx].product_id = e.target.value;
                        if (!newItems[idx].product_name_snapshot && e.target.value) {
                          const p = products.find((prod) => prod.product_id === e.target.value);
                          if (p) newItems[idx].product_name_snapshot = p.name;
                        }
                        setItems(newItems);
                      }}
                      style={{
                        width: "100%",
                        borderColor: !it.product_id ? "var(--danger)" : "var(--border)",
                        fontSize: "0.85rem",
                      }}
                    >
                      <option value="">-- Match Catalog Product --</option>
                      {products.map((p) => (
                        <option key={p.product_id} value={p.product_id}>
                          {p.name} {p.qr_code ? `(${p.qr_code})` : ""}
                        </option>
                      ))}
                    </select>
                  ) : (
                    <div className="muted" style={{ fontSize: "0.85rem" }}>
                      Product ID: {it.product_id}
                    </div>
                  )}
                  {it.confidence !== undefined && (
                    <span
                      style={{
                        display: "inline-block",
                        fontSize: "0.75rem",
                        marginTop: "0.2rem",
                        color: it.confidence >= 0.85 ? "var(--accent)" : "var(--danger)",
                      }}
                    >
                      OCR Confidence: {Math.round(it.confidence * 100)}%
                    </span>
                  )}
                </td>
                <td>
                  <input
                    type="number"
                    value={it.quantity_milli}
                    onChange={(e) => {
                      const newItems = [...items];
                      newItems[idx].quantity_milli = parseInt(e.target.value) || 0;
                      setItems(newItems);
                    }}
                    disabled={!isDraft}
                    style={{ width: "100%", textAlign: "right" }}
                  />
                </td>
                <td>
                  <input
                    type="number"
                    value={it.price_paise}
                    onChange={(e) => {
                      const newItems = [...items];
                      newItems[idx].price_paise = parseInt(e.target.value) || 0;
                      setItems(newItems);
                    }}
                    disabled={!isDraft}
                    style={{ width: "100%", textAlign: "right" }}
                  />
                </td>
                <td>
                  <input
                    type="number"
                    value={it.gst_rate_bps}
                    onChange={(e) => {
                      const newItems = [...items];
                      newItems[idx].gst_rate_bps = parseInt(e.target.value) || 0;
                      setItems(newItems);
                    }}
                    disabled={!isDraft}
                    style={{ width: "100%", textAlign: "right" }}
                  />
                </td>
                {isDraft && (
                  <td style={{ textAlign: "right" }}>
                    <button
                      className="btn btn-secondary"
                      onClick={() => setItems(items.filter((_, i) => i !== idx))}
                    >
                      &times;
                    </button>
                  </td>
                )}
              </tr>
            ))}
          </tbody>
        </table>

        {isDraft && (
          <div style={{ marginTop: "1rem", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
            <span className="muted" style={{ fontSize: "0.85rem" }}>
              Estimated Total: <strong>{formatPaise(tentativeGrandTotal)}</strong> (authoritative totals computed on save)
            </span>
            <button className="btn btn-secondary" onClick={handleSaveDraft} disabled={saving}>
              {saving ? "Saving..." : "Save Draft"}
            </button>
          </div>
        )}
      </div>

      {!isDraft && (
        <div className="card" style={{ marginTop: "2rem" }}>
          <div style={{ display: "flex", justifyContent: "flex-end", gap: "2rem", textAlign: "right" }}>
            <div>
              <p className="muted" style={{ margin: 0 }}>Subtotal</p>
              <h3>{formatPaise(purchase.subtotal_paise)}</h3>
            </div>
            <div>
              <p className="muted" style={{ margin: 0 }}>Tax</p>
              <h3>{formatPaise(purchase.tax_total_paise)}</h3>
            </div>
            <div>
              <p className="muted" style={{ margin: 0 }}>Grand Total</p>
              <h2 style={{ margin: 0, color: "var(--accent)" }}>{formatPaise(purchase.total_paise)}</h2>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
