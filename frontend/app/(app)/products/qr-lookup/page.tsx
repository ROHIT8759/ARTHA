"use client";

import { useState } from "react";
import Link from "next/link";
import { api, ProductView, formatPaise } from "@/lib/api";

export default function QrLookupPage() {
  const [qrCode, setQrCode] = useState("");
  const [product, setProduct] = useState<ProductView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const handleLookup = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!qrCode.trim()) return;
    
    setLoading(true);
    setError(null);
    setProduct(null);
    
    try {
      const data = await api.getProductByQr(qrCode.trim());
      setProduct(data);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Product not found for this QR code");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div>
      <div style={{ marginBottom: "2rem" }}>
        <Link href="/products" className="btn btn-secondary" style={{ marginBottom: "1rem", display: "inline-block" }}>
          &larr; Back to Products
        </Link>
        <h1 style={{ margin: 0 }}>QR Lookup Test</h1>
        <p className="muted">Simulate scanning a QR code to find a product.</p>
      </div>
      
      <form onSubmit={handleLookup} className="card" style={{ maxWidth: 500, marginBottom: "2rem" }}>
        <div className="field">
          <label>Scan or type QR Code</label>
          <div style={{ display: "flex", gap: "1rem" }}>
            <input 
              required 
              autoFocus
              value={qrCode} 
              onChange={e => setQrCode(e.target.value)} 
              placeholder="e.g. QR-RICE-1KG"
              style={{ flex: 1 }}
            />
            <button type="submit" className="btn" disabled={loading || !qrCode.trim()}>
              {loading ? "Looking up..." : "Lookup"}
            </button>
          </div>
        </div>
      </form>

      {error && (
        <div className="card" style={{ maxWidth: 500, borderColor: "var(--danger)", backgroundColor: "rgba(179, 38, 30, 0.05)" }}>
          <p style={{ color: "var(--danger)", margin: 0 }}>{error}</p>
        </div>
      )}

      {product && (
        <div className="card" style={{ maxWidth: 500 }}>
          <h3 style={{ marginTop: 0 }}>{product.name}</h3>
          
          {product.status === "archived" && (
            <div style={{ background: "#fce8e6", color: "#c5221f", padding: "0.5rem", borderRadius: 4, marginBottom: "1rem", fontSize: "0.9rem" }}>
              <strong>Warning:</strong> This product is currently inactive (archived).
            </div>
          )}
          
          <table style={{ width: "100%", fontSize: "0.95rem" }}>
            <tbody>
              <tr>
                <td style={{ color: "var(--muted)", padding: "0.5rem 0", borderBottom: "1px solid var(--border)" }}>QR Code</td>
                <td style={{ padding: "0.5rem 0", borderBottom: "1px solid var(--border)", textAlign: "right" }}><code>{product.qr_code}</code></td>
              </tr>
              <tr>
                <td style={{ color: "var(--muted)", padding: "0.5rem 0", borderBottom: "1px solid var(--border)" }}>Price</td>
                <td style={{ padding: "0.5rem 0", borderBottom: "1px solid var(--border)", textAlign: "right", fontWeight: 600 }}>{formatPaise(product.price_paise)}</td>
              </tr>
              <tr>
                <td style={{ color: "var(--muted)", padding: "0.5rem 0", borderBottom: "1px solid var(--border)" }}>Batch</td>
                <td style={{ padding: "0.5rem 0", borderBottom: "1px solid var(--border)", textAlign: "right" }}>{product.batch || "—"}</td>
              </tr>
              <tr>
                <td style={{ color: "var(--muted)", padding: "0.5rem 0", borderBottom: "1px solid var(--border)" }}>HSN Code</td>
                <td style={{ padding: "0.5rem 0", borderBottom: "1px solid var(--border)", textAlign: "right" }}>{product.hsn_code || "—"}</td>
              </tr>
              <tr>
                <td style={{ color: "var(--muted)", padding: "0.5rem 0", borderBottom: "1px solid var(--border)" }}>GST Rate</td>
                <td style={{ padding: "0.5rem 0", borderBottom: "1px solid var(--border)", textAlign: "right" }}>{(product.gst_rate_bps / 100).toFixed(2)}%</td>
              </tr>
            </tbody>
          </table>
          
          <div style={{ marginTop: "1.5rem", textAlign: "right" }}>
            <Link href={`/products/${product.product_id}`} className="btn btn-secondary">
              Edit Product
            </Link>
          </div>
        </div>
      )}
    </div>
  );
}
