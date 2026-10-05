"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { api, formatPaise } from "@/lib/api";
import LoadingState from "@/components/ui/LoadingState";
import ErrorState from "@/components/ui/ErrorState";

type PurchaseView = {
  purchase_id: string;
  supplier_id?: string;
  invoice_no?: string;
  invoice_date?: string;
  status: string;
  source: string;
  total_paise: number;
  created_at: string;
};

export default function PurchasesPage() {
  const [purchases, setPurchases] = useState<PurchaseView[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let mounted = true;
    const fetchPurchases = async () => {
      try {
        setLoading(true);
        const data = await api.listPurchases();
        if (mounted) {
          setPurchases(data);
          setError(null);
        }
      } catch (err: unknown) {
        if (mounted) setError(err instanceof Error ? err.message : "Failed to load purchases");
      } finally {
        if (mounted) setLoading(false);
      }
    };
    fetchPurchases();
    return () => { mounted = false; };
  }, []);

  return (
    <div>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "2rem" }}>
        <h1 style={{ margin: 0 }}>Purchases</h1>
        <Link href="/purchases/new" className="btn">
          + New Purchase
        </Link>
      </div>

      {error ? (
        <ErrorState message={error} onRetry={() => window.location.reload()} />
      ) : loading ? (
        <LoadingState />
      ) : purchases.length === 0 ? (
        <div style={{ textAlign: "center", padding: "3rem", background: "var(--border)", borderRadius: 8 }}>
          <p>No purchases found.</p>
        </div>
      ) : (
        <div style={{ overflowX: "auto" }}>
          <table>
            <thead>
              <tr>
                <th>Date</th>
                <th>Invoice No</th>
                <th>Total</th>
                <th>Source</th>
                <th>Status</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {purchases.map(p => (
                <tr key={p.purchase_id} style={{ opacity: p.status === "void" ? 0.6 : 1 }}>
                  <td>{new Date(p.created_at).toLocaleDateString()}</td>
                  <td>{p.invoice_no || "—"}</td>
                  <td style={{ fontWeight: 600 }}>{formatPaise(p.total_paise)}</td>
                  <td style={{ textTransform: "capitalize" }}>{p.source}</td>
                  <td>
                    <span style={{ 
                      padding: "0.2rem 0.5rem", 
                      borderRadius: 99, 
                      fontSize: "0.8rem",
                      background: p.status === "approved" ? "#e6f4ea" : p.status === "void" ? "#fce8e6" : "#fef7e0",
                      color: p.status === "approved" ? "#137333" : p.status === "void" ? "#c5221f" : "#b06000"
                    }}>
                      {p.status}
                    </span>
                  </td>
                  <td style={{ textAlign: "right" }}>
                    <Link href={`/purchases/${p.purchase_id}`} style={{ color: "var(--accent)", textDecoration: "none" }}>
                      {p.status === "draft" ? "Review / Edit" : "View"}
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
