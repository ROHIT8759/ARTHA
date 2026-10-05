"use client";

import { useEffect, useState } from "react";
import { useParams } from "next/navigation";
import Link from "next/link";
import { api, ProductView, ApiError } from "@/lib/api";
import LoadingState from "@/components/ui/LoadingState";
import ErrorState from "@/components/ui/ErrorState";

export default function InventoryDetailPage() {
  const params = useParams();
  const productId = params.id as string;
  
  const [product, setProduct] = useState<ProductView | null>(null);
  const [stock, setStock] = useState<number | null>(null);
  const [history, setHistory] = useState<Array<{
    transaction_id: string;
    product_id: string;
    transaction_type: string;
    quantity_milli: number;
    reference_id?: string;
    reason?: string;
    created_by: string;
    created_at: string;
  }>>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [qty, setQty] = useState("");
  const [reason, setReason] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  useEffect(() => {
    let mounted = true;
    const fetchData = async () => {
      try {
        setLoading(true);
        const [prodRes, statRes, histRes] = await Promise.all([
          api.getProduct(productId),
          api.getInventoryStatus(productId),
          api.getInventoryHistory(productId)
        ]);
        if (mounted) {
          setProduct(prodRes);
          setStock(statRes.current_stock_milli);
          setHistory(histRes);
          setError(null);
        }
      } catch (err: unknown) {
        if (mounted) setError(err instanceof Error ? err.message : "Failed to load inventory data");
      } finally {
        if (mounted) setLoading(false);
      }
    };
    fetchData();
    return () => { mounted = false; };
  }, [productId]);

  const refreshData = async () => {
    try {
      const [prodRes, statRes, histRes] = await Promise.all([
        api.getProduct(productId),
        api.getInventoryStatus(productId),
        api.getInventoryHistory(productId)
      ]);
      setProduct(prodRes);
      setStock(statRes.current_stock_milli);
      setHistory(histRes);
    } catch (err: unknown) {
      console.error(err);
    }
  };

  const handleAction = async (type: "opening" | "adjust") => {
    if (!qty) return;
    setSubmitting(true);
    setActionError(null);
    try {
      const q = Math.round(parseFloat(qty) * 1000);
      if (type === "opening") {
        await api.postOpeningStock(productId, q);
      } else {
        await api.postAdjustment(productId, q, reason);
      }
      setQty("");
      setReason("");
      await refreshData(); // refresh data
    } catch (err: unknown) {
      setActionError(err instanceof ApiError ? err.message : err instanceof Error ? err.message : "Action failed");
    } finally {
      setSubmitting(false);
    }
  };

  if (loading && !product) return <LoadingState />;
  if (error) return <ErrorState message={error} onRetry={() => window.location.reload()} />;
  if (!product) return null;

  const hasOpening = history.some(h => h.transaction_type === "opening");
  const isArchived = product.status === "archived";

  return (
    <div>
      <div style={{ marginBottom: "2rem" }}>
        <Link href="/inventory" className="btn btn-secondary" style={{ marginBottom: "1rem", display: "inline-block" }}>
          &larr; Back to Inventory
        </Link>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-end" }}>
          <div>
            <h1 style={{ margin: 0 }}>{product.name}</h1>
            <p className="muted" style={{ margin: "0.5rem 0 0" }}>Batch: {product.batch || "—"} | QR: {product.qr_code}</p>
          </div>
          <div style={{ textAlign: "right" }}>
            <p className="muted" style={{ margin: 0 }}>Current Stock</p>
            <h2 style={{ margin: 0 }}>{(stock ?? 0) / 1000} {product.unit}</h2>
          </div>
        </div>
      </div>

      {isArchived && (
        <div style={{ background: "#fce8e6", color: "#c5221f", padding: "1rem", borderRadius: 8, marginBottom: "2rem" }}>
          <strong>Product is Archived:</strong> Inventory transactions cannot be created for archived products.
        </div>
      )}

      {!isArchived && (
        <div className="card" style={{ marginBottom: "2rem" }}>
          <h3 style={{ marginTop: 0 }}>Record Movement</h3>
          <div style={{ display: "flex", gap: "1rem", alignItems: "flex-end", flexWrap: "wrap" }}>
            <div className="field" style={{ flex: 1, minWidth: "150px" }}>
              <label>Quantity ({product.unit})</label>
              <input 
                type="number" 
                step="0.001" 
                value={qty} 
                onChange={e => setQty(e.target.value)} 
                placeholder={hasOpening ? "e.g. -5 or +10" : "Initial quantity"}
              />
            </div>
            
            {hasOpening && (
              <div className="field" style={{ flex: 2, minWidth: "200px" }}>
                <label>Reason for adjustment *</label>
                <input 
                  type="text" 
                  value={reason} 
                  onChange={e => setReason(e.target.value)} 
                  placeholder="e.g. Damaged goods, Stock check"
                />
              </div>
            )}

            <div style={{ paddingBottom: "0.2rem" }}>
              {!hasOpening ? (
                <button 
                  className="btn" 
                  onClick={() => handleAction("opening")} 
                  disabled={submitting || !qty}
                >
                  Set Opening Stock
                </button>
              ) : (
                <button 
                  className="btn" 
                  onClick={() => handleAction("adjust")} 
                  disabled={submitting || !qty || !reason}
                >
                  Adjust Stock
                </button>
              )}
            </div>
          </div>
          {actionError && <p className="error-text" style={{ marginTop: "1rem" }}>{actionError}</p>}
        </div>
      )}

      <div className="card">
        <h3 style={{ marginTop: 0 }}>Transaction Ledger</h3>
        {history.length === 0 ? (
          <p className="muted">No inventory history found.</p>
        ) : (
          <table style={{ width: "100%", fontSize: "0.95rem" }}>
            <thead>
              <tr>
                <th style={{ textAlign: "left" }}>Date</th>
                <th style={{ textAlign: "left" }}>Type</th>
                <th style={{ textAlign: "left" }}>Reason</th>
                <th style={{ textAlign: "right" }}>Quantity</th>
              </tr>
            </thead>
            <tbody>
              {history.map(h => (
                <tr key={h.transaction_id}>
                  <td style={{ padding: "0.75rem 0", borderBottom: "1px solid var(--border)" }}>
                    {new Date(h.created_at).toLocaleString()}
                  </td>
                  <td style={{ padding: "0.75rem 0", borderBottom: "1px solid var(--border)", textTransform: "capitalize" }}>
                    {h.transaction_type.replace("_", " ")}
                  </td>
                  <td style={{ padding: "0.75rem 0", borderBottom: "1px solid var(--border)", color: "var(--muted)" }}>
                    {h.reason || h.reference_id || "—"}
                  </td>
                  <td style={{ 
                    padding: "0.75rem 0", 
                    borderBottom: "1px solid var(--border)", 
                    textAlign: "right",
                    color: h.quantity_milli > 0 ? "var(--accent)" : "var(--danger)",
                    fontWeight: 600
                  }}>
                    {h.quantity_milli > 0 ? "+" : ""}{h.quantity_milli / 1000} {product.unit}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
