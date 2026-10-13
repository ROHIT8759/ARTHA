"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { api, SaleView, formatPaise } from "@/lib/api";
import EmptyState from "@/components/ui/EmptyState";
import LoadingState from "@/components/ui/LoadingState";
import ErrorState from "@/components/ui/ErrorState";

type DraftPurchase = {
  purchase_id: string;
  invoice_no?: string;
  total_paise: number;
  created_at: string;
};

/**
 * Recoverable-drafts summary (Phase 09 / V0.3 Reliability's "list and resume
 * unfinished work" requirement). Resuming and safely abandoning a draft
 * already live on each draft's own edit page (the "Discard"/"Void Draft"
 * button there) — this view's job is just to surface what's recoverable
 * across both sales and purchases in one place and link out to it, not to
 * duplicate that action here.
 */
export default function DashboardPage() {
  const [draftSales, setDraftSales] = useState<SaleView[]>([]);
  const [draftPurchases, setDraftPurchases] = useState<DraftPurchase[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let mounted = true;
    const loadDrafts = async () => {
      try {
        setLoading(true);
        const [sales, purchases] = await Promise.all([
          api.listSales({ status: "draft" }),
          api.listPurchases({ status: "draft" }),
        ]);
        if (mounted) {
          setDraftSales(sales);
          setDraftPurchases(purchases);
          setError(null);
        }
      } catch (err: unknown) {
        if (mounted) {
          setError(err instanceof Error ? err.message : "Failed to load unfinished work");
        }
      } finally {
        if (mounted) setLoading(false);
      }
    };
    loadDrafts();
    return () => {
      mounted = false;
    };
  }, []);

  const hasDrafts = draftSales.length > 0 || draftPurchases.length > 0;

  return (
    <div>
      <h1>Dashboard</h1>
      <p className="muted" style={{ marginBottom: "2rem" }}>Overview of your business</p>

      <div className="card" style={{ marginBottom: "2rem" }}>
        <h2 style={{ fontSize: "1.2rem", marginTop: 0, borderBottom: "1px solid var(--border)", paddingBottom: "0.5rem" }}>
          📋 Resume Unfinished Work
        </h2>

        {loading && <LoadingState message="Checking for unfinished sales and purchases..." />}
        {error && !loading && <ErrorState message={error} onRetry={() => window.location.reload()} />}

        {!loading && !error && !hasDrafts && (
          <p className="muted" style={{ margin: 0 }}>
            Nothing left unfinished — every draft sale and purchase has been completed or discarded.
          </p>
        )}

        {!loading && !error && hasDrafts && (
          <div style={{ display: "grid", gap: "1.5rem" }}>
            {draftSales.length > 0 && (
              <div>
                <h3 style={{ fontSize: "0.95rem", margin: "0 0 0.5rem" }}>
                  Draft Sales ({draftSales.length})
                </h3>
                <div style={{ display: "flex", flexDirection: "column", gap: "0.5rem" }}>
                  {draftSales.map((sale) => (
                    <div
                      key={sale.sale_id}
                      style={{
                        display: "flex",
                        justifyContent: "space-between",
                        alignItems: "center",
                        padding: "0.6rem 0.75rem",
                        borderRadius: 6,
                        border: "1px solid var(--border)",
                      }}
                    >
                      <div>
                        <div>{sale.customer_name || "Walk-in customer"}</div>
                        <div className="muted" style={{ fontSize: "0.8rem" }}>
                          Started {new Date(sale.created_at).toLocaleString()} · {formatPaise(sale.grand_total_paise)}
                        </div>
                      </div>
                      <Link href={`/sales/${sale.sale_id}`} className="btn btn-secondary" style={{ padding: "0.3rem 0.7rem", fontSize: "0.85rem" }}>
                        Resume
                      </Link>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {draftPurchases.length > 0 && (
              <div>
                <h3 style={{ fontSize: "0.95rem", margin: "0 0 0.5rem" }}>
                  Draft Purchases ({draftPurchases.length})
                </h3>
                <div style={{ display: "flex", flexDirection: "column", gap: "0.5rem" }}>
                  {draftPurchases.map((purchase) => (
                    <div
                      key={purchase.purchase_id}
                      style={{
                        display: "flex",
                        justifyContent: "space-between",
                        alignItems: "center",
                        padding: "0.6rem 0.75rem",
                        borderRadius: 6,
                        border: "1px solid var(--border)",
                      }}
                    >
                      <div>
                        <div>{purchase.invoice_no || "No invoice number yet"}</div>
                        <div className="muted" style={{ fontSize: "0.8rem" }}>
                          Started {new Date(purchase.created_at).toLocaleString()} · {formatPaise(purchase.total_paise)}
                        </div>
                      </div>
                      <Link href={`/purchases/${purchase.purchase_id}`} className="btn btn-secondary" style={{ padding: "0.3rem 0.7rem", fontSize: "0.85rem" }}>
                        Resume
                      </Link>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>
        )}
      </div>

      <EmptyState
        title="Business metrics not implemented yet"
        description="This section will show revenue, recent activity, and low stock alerts. It will be built in a future phase."
      />
    </div>
  );
}
