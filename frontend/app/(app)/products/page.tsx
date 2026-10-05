"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { api, ProductView, formatPaise } from "@/lib/api";
import LoadingState from "@/components/ui/LoadingState";
import ErrorState from "@/components/ui/ErrorState";

export default function ProductsPage() {
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
        if (mounted) setError(err instanceof Error ? err.message : "Failed to load products");
      } finally {
        if (mounted) setLoading(false);
      }
    };

    const timer = setTimeout(fetchProducts, 300); // debounce search
    return () => {
      mounted = false;
      clearTimeout(timer);
    };
  }, [search]);

  return (
    <div>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "2rem" }}>
        <h1 style={{ margin: 0 }}>Products</h1>
        <div style={{ display: "flex", gap: "1rem" }}>
          <Link href="/products/qr-lookup" className="btn btn-secondary">
            Test QR Lookup
          </Link>
          <Link href="/products/new" className="btn">
            + New Product
          </Link>
        </div>
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
                <th>Name</th>
                <th>Batch</th>
                <th>QR Code</th>
                <th>Price</th>
                <th>Stock</th>
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
                  <td>{formatPaise(p.price_paise)}</td>
                  <td>{p.stock_qty_milli / 1000} {p.unit}</td>
                  <td>
                    <span style={{ 
                      padding: "0.2rem 0.5rem", 
                      borderRadius: 99, 
                      fontSize: "0.8rem",
                      background: p.status === "active" ? "#e6f4ea" : "#fce8e6",
                      color: p.status === "active" ? "#137333" : "#c5221f"
                    }}>
                      {p.status}
                    </span>
                  </td>
                  <td style={{ textAlign: "right" }}>
                    <Link href={`/products/${p.product_id}`} style={{ color: "var(--accent)", textDecoration: "none" }}>
                      Edit
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
