"use client";

import { useEffect, useState, useRef } from "react";
import { useParams, useRouter } from "next/navigation";
import Link from "next/link";
import { api, formatPaise } from "@/lib/api";
import LoadingState from "@/components/ui/LoadingState";
import ErrorState from "@/components/ui/ErrorState";

export default function PurchaseDetailPage() {
  const params = useParams();
  const purchaseId = params.id as string;
  const router = useRouter();
  
  const [purchase, setPurchase] = useState<{
    status: string;
    source: string;
    subtotal_paise: number;
    tax_total_paise: number;
    total_paise: number;
  } | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Draft Edit State
  const [invoiceNo, setInvoiceNo] = useState("");
  const [items, setItems] = useState<Array<{
    purchase_item_id?: string;
    product_id?: string;
    product_name_snapshot?: string;
    quantity_milli: number;
    price_paise: number;
    gst_rate_bps: number;
  }>>([]);
  const [saving, setSaving] = useState(false);
  const [approving, setApproving] = useState(false);
  const [ocrProcessing, setOcrProcessing] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    let mounted = true;
    const fetchPurchase = async () => {
      try {
        setLoading(true);
        const data = await api.getPurchase(purchaseId);
        if (mounted) {
          setPurchase(data);
          setInvoiceNo(data.invoice_no || "");
          setItems(data.items || []);
          setError(null);
        }
      } catch (err: unknown) {
        if (mounted) setError(err instanceof Error ? err.message : "Failed to load purchase");
      } finally {
        if (mounted) setLoading(false);
      }
    };
    fetchPurchase();
    return () => { mounted = false; };
  }, [purchaseId]);

  const handleSaveDraft = async () => {
    setSaving(true);
    setError(null);
    try {
      await api.updatePurchase(purchaseId, {
        invoice_no: invoiceNo,
        items: items
      });
      // fetch again to get updated state
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
      await handleSaveDraft(); // Save first
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
      
      // Apply parsed OCR data to our draft state
      if (ocrRes.parsed) {
        setInvoiceNo(ocrRes.parsed.invoice_no || invoiceNo);
        setItems((ocrRes.parsed.items || []).map(i => ({
          product_name_snapshot: String(i.product_name_snapshot || ""),
          quantity_milli: Number(i.quantity_milli || 1000),
          price_paise: Number(i.price_paise || 0),
          gst_rate_bps: Number(i.gst_rate_bps || 0)
        })));
      }
      alert("Smart Scan completed. Please review the extracted data.");
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
  const hasUnresolved = items.some(i => !i.product_id);

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
            <p className="muted" style={{ margin: "0.5rem 0 0", textTransform: "capitalize" }}>Status: {purchase.status} | Source: {purchase.source}</p>
          </div>
          {isDraft && (
            <div style={{ display: "flex", gap: "1rem" }}>
              <button className="btn btn-secondary" onClick={handleCancel} style={{ borderColor: "var(--danger)", color: "var(--danger)" }}>
                Void Draft
              </button>
              <button className="btn" onClick={handleApprove} disabled={approving || saving || hasUnresolved}>
                {approving ? "Approving..." : "Approve & Update Inventory"}
              </button>
            </div>
          )}
        </div>
      </div>
      
      {error && (
        <div className="card" style={{ marginBottom: "1rem", borderColor: "var(--danger)", backgroundColor: "rgba(179, 38, 30, 0.05)" }}>
          <p style={{ color: "var(--danger)", margin: 0 }}>{error}</p>
        </div>
      )}

      {isDraft && purchase.source === "ocr" && items.length === 0 && (
        <div className="card" style={{ marginBottom: "2rem", textAlign: "center", border: "2px dashed var(--accent)" }}>
          <h3>Upload Bill Photo</h3>
          <p className="muted">Upload an image of the invoice to extract items automatically.</p>
          <input 
            type="file" 
            accept="image/*" 
            onChange={handleFileUpload} 
            ref={fileInputRef}
            style={{ display: "none" }}
          />
          <button className="btn" onClick={() => fileInputRef.current?.click()} disabled={ocrProcessing}>
            {ocrProcessing ? "Processing (may take 5-10s)..." : "Select Image & Process"}
          </button>
        </div>
      )}

      <div className="card" style={{ marginBottom: "2rem" }}>
        <h3>Invoice Metadata</h3>
        <div className="field" style={{ maxWidth: 300 }}>
          <label>Invoice Number</label>
          <input 
            value={invoiceNo} 
            onChange={e => setInvoiceNo(e.target.value)} 
            disabled={!isDraft}
            placeholder="e.g. INV-2023-01"
          />
        </div>
      </div>

      <div className="card">
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <h3 style={{ margin: 0 }}>Line Items</h3>
          {isDraft && (
            <button className="btn btn-secondary" onClick={() => setItems([...items, { product_id: "", product_name_snapshot: "", quantity_milli: 1000, price_paise: 0, gst_rate_bps: 0 }])}>
              + Add Item
            </button>
          )}
        </div>
        
        {hasUnresolved && isDraft && (
           <div style={{ background: "#fef7e0", color: "#b06000", padding: "0.5rem", borderRadius: 4, marginTop: "1rem", fontSize: "0.9rem" }}>
             <strong>Warning:</strong> You have items without a linked product. Please resolve them before approval.
           </div>
        )}

        <table style={{ width: "100%", marginTop: "1rem" }}>
          <thead>
            <tr>
              <th style={{ textAlign: "left" }}>Product Name / ID (Matched)</th>
              <th style={{ textAlign: "right" }}>Qty (Milli)</th>
              <th style={{ textAlign: "right" }}>Price (Paise)</th>
              <th style={{ textAlign: "right" }}>GST (Bps)</th>
              {isDraft && <th></th>}
            </tr>
          </thead>
          <tbody>
            {items.map((it, idx) => (
              <tr key={idx} style={{ background: !it.product_id ? "rgba(179, 38, 30, 0.05)" : "transparent" }}>
                <td>
                  <input 
                    value={it.product_name_snapshot || ""} 
                    onChange={e => {
                      const newItems = [...items];
                      newItems[idx].product_name_snapshot = e.target.value;
                      setItems(newItems);
                    }}
                    disabled={!isDraft}
                    placeholder="Extracted Name"
                    style={{ width: "100%", marginBottom: "0.5rem" }}
                  />
                  <input 
                    value={it.product_id || ""} 
                    onChange={e => {
                      const newItems = [...items];
                      newItems[idx].product_id = e.target.value;
                      setItems(newItems);
                    }}
                    disabled={!isDraft}
                    placeholder="Enter Product ID to match..."
                    style={{ width: "100%", borderColor: !it.product_id ? "var(--danger)" : "var(--border)" }}
                  />
                </td>
                <td>
                  <input 
                    type="number" 
                    value={it.quantity_milli} 
                    onChange={e => {
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
                    onChange={e => {
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
                    onChange={e => {
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
                    <button className="btn btn-secondary" onClick={() => setItems(items.filter((_, i) => i !== idx))}>
                      &times;
                    </button>
                  </td>
                )}
              </tr>
            ))}
          </tbody>
        </table>
        
        {isDraft && (
          <div style={{ marginTop: "1rem", textAlign: "right" }}>
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
