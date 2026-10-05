"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { api, ProductView } from "@/lib/api";
import LoadingState from "@/components/ui/LoadingState";
import ErrorState from "@/components/ui/ErrorState";

export default function InventoryPage() {
  const [products, setProducts] = useState<ProductView[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");

  useEffect(() => {
    let mounted = true;
    const fetchProducts = async () => {
      try {
        setLoading(true);
        const data = await api.listProducts(search);
        if (mounted) {
          setProducts(data);
          setError(null);
        }
      } catch (err: unknown) {
        if (mounted) setError(err instanceof Error ? err.message : "Failed to load inventory");
      } finally {
        if (mounted) setLoading(false);
      }
    };

    const timer = setTimeout(fetchProducts, 300);
    return () => {
      mounted = false;
      clearTimeout(timer);
    };
  }, [search]);

  return (
    <div>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "2rem" }}>
        <h1 style={{ margin: 0 }}>Inventory Overview</h1>
      </div>
      
      <div className="field" style={{ maxWidth: 400, marginBottom: "2rem" }}>
        <input 
          type="search" 
          placeholder="Search by name, batch, or QR..." 
          value={search}
          onChange={e => setSearch(e.target.value)}
        />
      </div>

      {error ? (
        <ErrorState message={error} onRetry={() => setSearch(search)} />
      ) : loading && products.length === 0 ? (
        <LoadingState />
      ) : products.length === 0 ? (
        <div style={{ textAlign: "center", padding: "3rem", background: "var(--border)", borderRadius: 8 }}>
          <p>No products found.</p>
        </div>
      ) : (
        <div style={{ overflowX: "auto" }}>
          <table>
            <thead>
              <tr>
                <th>Product</th>
                <th>Batch</th>
                <th>QR Code</th>
                <th>Current Stock</th>
                <th>Status</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {products.map(p => (
                <tr key={p.product_id} style={{ opacity: p.status === "archived" ? 0.6 : 1 }}>
                  <td>{p.name}</td>
                  <td>{p.batch || "—"}</td>
                  <td><code>{p.qr_code}</code></td>
                  <td style={{ fontWeight: 600, color: p.stock_qty_milli < 0 ? "var(--danger)" : "inherit" }}>
                    {p.stock_qty_milli / 1000} {p.unit}
                  </td>
                  <td>
                    {p.status === "active" ? "Active" : "Archived"}
                  </td>
                  <td style={{ textAlign: "right" }}>
                    <Link href={`/inventory/${p.product_id}`} className="btn btn-secondary">
                      Manage Stock
                    </Link>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
