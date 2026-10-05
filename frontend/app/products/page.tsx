"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import { ApiError, api, formatPaise, type ProductView } from "@/lib/api";
import { useAuth } from "@/lib/auth-context";

export default function ProductsPage() {
  const router = useRouter();
  const { user, loading } = useAuth();
  const [products, setProducts] = useState<ProductView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showForm, setShowForm] = useState(false);

  useEffect(() => {
    if (!loading && !user) router.replace("/login");
  }, [loading, user, router]);

  async function reload() {
    try {
      setProducts(await api.listProducts());
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Could not load products.");
    }
  }

  useEffect(() => {
    if (user) reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [user]);

  if (loading || !user) return null;

  return (
    <main style={{ padding: "2rem", maxWidth: 900, margin: "0 auto" }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline" }}>
        <div>
          <Link href="/dashboard" className="muted">
            ← Dashboard
          </Link>
          <h1 style={{ marginTop: "0.25rem" }}>Products</h1>
        </div>
        <button className="btn" onClick={() => setShowForm((v) => !v)}>
          {showForm ? "Close" : "Add product"}
        </button>
      </div>

      {error && <p className="error-text">{error}</p>}

      {showForm && (
        <NewProductForm
          onCreated={() => {
            setShowForm(false);
            reload();
          }}
        />
      )}

      {products === null ? (
        <p className="muted">Loading…</p>
      ) : products.length === 0 ? (
        <p className="muted">No products yet. Add one to generate its QR identity.</p>
      ) : (
        <table style={{ marginTop: "1.5rem" }}>
          <thead>
            <tr>
              <th>Name</th>
              <th>QR code</th>
              <th>Price</th>
              <th>Stock</th>
              <th>Status</th>
            </tr>
          </thead>
          <tbody>
            {products.map((p) => (
              <tr key={p.product_id}>
                <td>{p.name}</td>
                <td>
                  <code>{p.qr_code}</code>
                </td>
                <td>{formatPaise(p.price_paise)}</td>
                <td>{(p.stock_qty_milli / 1000).toFixed(3)} {p.unit}</td>
                <td>{p.status}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </main>
  );
}

function NewProductForm({ onCreated }: { onCreated: () => void }) {
  const [name, setName] = useState("");
  const [qrCode, setQrCode] = useState("");
  const [price, setPrice] = useState("");
  const [cost, setCost] = useState("");
  const [gstPercent, setGstPercent] = useState("0");
  const [unit, setUnit] = useState("pcs");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setSubmitting(true);
    try {
      await api.createProduct({
        name: name.trim(),
        qr_code: qrCode.trim(),
        // Rupees (decimal string from the form) -> integer paise for storage.
        price_paise: Math.round(parseFloat(price || "0") * 100),
        cost_paise: Math.round(parseFloat(cost || "0") * 100),
        // GST % (e.g. "18") -> basis points (1800) for storage.
        gst_rate_bps: Math.round(parseFloat(gstPercent || "0") * 100),
        unit: unit.trim() || "pcs",
      });
      onCreated();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Could not create product.");
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <form onSubmit={onSubmit} className="card" style={{ marginTop: "1.5rem" }}>
      <div className="field">
        <label htmlFor="name">Name</label>
        <input id="name" required value={name} onChange={(e) => setName(e.target.value)} />
      </div>
      <div className="field">
        <label htmlFor="qrCode">QR code (scan or type the code to assign)</label>
        <input id="qrCode" required value={qrCode} onChange={(e) => setQrCode(e.target.value)} />
      </div>
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: "1rem" }}>
        <div className="field">
          <label htmlFor="price">Price (₹)</label>
          <input id="price" type="number" step="0.01" min="0" required value={price} onChange={(e) => setPrice(e.target.value)} />
        </div>
        <div className="field">
          <label htmlFor="cost">Cost (₹)</label>
          <input id="cost" type="number" step="0.01" min="0" value={cost} onChange={(e) => setCost(e.target.value)} />
        </div>
        <div className="field">
          <label htmlFor="gst">GST %</label>
          <input id="gst" type="number" step="0.01" min="0" value={gstPercent} onChange={(e) => setGstPercent(e.target.value)} />
        </div>
      </div>
      <div className="field">
        <label htmlFor="unit">Unit</label>
        <input id="unit" value={unit} onChange={(e) => setUnit(e.target.value)} />
      </div>

      {error && <p className="error-text">{error}</p>}

      <button className="btn" type="submit" disabled={submitting}>
        {submitting ? "Saving…" : "Save product"}
      </button>
    </form>
  );
}
