"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { api, SaleView, formatPaise } from "@/lib/api";
import LoadingState from "@/components/ui/LoadingState";
import ErrorState from "@/components/ui/ErrorState";
import EmptyState from "@/components/ui/EmptyState";

export default function SalesPage() {
  const router = useRouter();
  const [sales, setSales] = useState<SaleView[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [statusFilter, setStatusFilter] = useState<string>("all");
  const [startingNewSale, setStartingNewSale] = useState(false);

  useEffect(() => {
    let mounted = true;
    const fetchSales = async () => {
      try {
        setLoading(true);
        const query: { search?: string; status?: string } = {};
        if (search.trim()) query.search = search.trim();
        if (statusFilter !== "all") query.status = statusFilter;

        const data = await api.listSales(query);
        if (mounted) {
          setSales(data);
          setError(null);
        }
      } catch (err: unknown) {
        if (mounted) {
          setError(err instanceof Error ? err.message : "Failed to load sales");
        }
      } finally {
        if (mounted) {
          setLoading(false);
        }
      }
    };

    fetchSales();
    return () => {
      mounted = false;
    };
  }, [statusFilter, search]);

  const handleSearchSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    try {
      setLoading(true);
      const query: { search?: string; status?: string } = {};
      if (search.trim()) query.search = search.trim();
      if (statusFilter !== "all") query.status = statusFilter;
      const data = await api.listSales(query);
      setSales(data);
      setError(null);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to search sales");
    } finally {
      setLoading(false);
    }
  };

  const handleQuickNewSale = async () => {
    setStartingNewSale(true);
    try {
      const draft = await api.createSale({ payment_method: "cash" });
      router.push(`/sales/${draft.sale_id}`);
    } catch (err: unknown) {
      alert(err instanceof Error ? err.message : "Failed to create new sale draft");
      setStartingNewSale(false);
    }
  };

  return (
    <div>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "1.5rem" }}>
        <div>
          <h1 style={{ margin: 0 }}>Sales & Billing</h1>
          <p className="muted" style={{ margin: "0.25rem 0 0" }}>Manage checkout, invoices, and payment history</p>
        </div>
        <div style={{ display: "flex", gap: "0.75rem" }}>
          <button
            onClick={handleQuickNewSale}
            className="btn"
            disabled={startingNewSale}
            style={{ display: "flex", alignItems: "center", gap: "0.5rem" }}
          >
            {startingNewSale ? "Starting POS..." : "⚡ New Sale / POS"}
          </button>
        </div>
      </div>

      <div className="card" style={{ marginBottom: "1.5rem", padding: "1rem" }}>
        <form onSubmit={handleSearchSubmit} style={{ display: "flex", gap: "1rem", flexWrap: "wrap", alignItems: "center" }}>
          <div style={{ flex: 1, minWidth: "200px" }}>
            <input
              type="text"
              placeholder="Search by invoice #, customer name or phone..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              style={{ width: "100%", padding: "0.55rem 0.75rem", borderRadius: "6px", border: "1px solid var(--border)", background: "var(--background)", color: "var(--foreground)" }}
            />
          </div>

          <div style={{ display: "flex", alignItems: "center", gap: "0.5rem" }}>
            <label className="muted" style={{ fontSize: "0.9rem" }}>Status:</label>
            <select
              value={statusFilter}
              onChange={(e) => setStatusFilter(e.target.value)}
              style={{ padding: "0.55rem 0.75rem", borderRadius: "6px", border: "1px solid var(--border)", background: "var(--background)", color: "var(--foreground)" }}
            >
              <option value="all">All Sales</option>
              <option value="completed">Completed</option>
              <option value="draft">Drafts</option>
              <option value="void">Void</option>
            </select>
          </div>

          <button type="submit" className="btn btn-secondary">
            Search
          </button>
        </form>
      </div>

      {loading && <LoadingState />}
      {error && !loading && <ErrorState message={error} onRetry={() => window.location.reload()} />}

      {!loading && !error && sales.length === 0 && (
        <EmptyState
          title="No sales found"
          description={search ? "No sales match your current search query." : "No sales recorded yet. Start your first checkout!"}
          actionLabel="Start New Sale"
          onAction={handleQuickNewSale}
        />
      )}

      {!loading && !error && sales.length > 0 && (
        <div className="card" style={{ padding: 0, overflowX: "auto" }}>
          <table style={{ margin: 0 }}>
            <thead>
              <tr style={{ background: "rgba(0,0,0,0.02)" }}>
                <th>Invoice #</th>
                <th>Date / Time</th>
                <th>Customer</th>
                <th>Payment</th>
                <th style={{ textAlign: "right" }}>Amount</th>
                <th style={{ textAlign: "center" }}>Status</th>
                <th style={{ textAlign: "right" }}>Actions</th>
              </tr>
            </thead>
            <tbody>
              {sales.map((sale) => {
                const dateStr = new Date(sale.created_at).toLocaleString();
                const statusBg =
                  sale.status === "completed"
                    ? "rgba(15, 107, 76, 0.12)"
                    : sale.status === "draft"
                    ? "rgba(224, 160, 0, 0.15)"
                    : "rgba(179, 38, 30, 0.12)";
                const statusColor =
                  sale.status === "completed"
                    ? "var(--accent)"
                    : sale.status === "draft"
                    ? "#b27b00"
                    : "var(--danger)";

                return (
                  <tr key={sale.sale_id}>
                    <td>
                      {sale.invoice_no ? (
                        <strong style={{ fontFamily: "monospace" }}>{sale.invoice_no}</strong>
                      ) : (
                        <span className="muted" style={{ fontStyle: "italic" }}>Draft</span>
                      )}
                    </td>
                    <td className="muted" style={{ fontSize: "0.9rem" }}>{dateStr}</td>
                    <td>
                      {sale.customer_name ? (
                        <div>
                          <div>{sale.customer_name}</div>
                          {sale.customer_phone && (
                            <div className="muted" style={{ fontSize: "0.8rem" }}>{sale.customer_phone}</div>
                          )}
                        </div>
                      ) : (
                        <span className="muted">—</span>
                      )}
                    </td>
                    <td>
                      <span style={{ textTransform: "uppercase", fontSize: "0.85rem", fontWeight: 600 }}>
                        {sale.payment_method}
                      </span>
                      {sale.payment_status === "paid" && (
                        <span style={{ marginLeft: "0.4rem", color: "var(--accent)", fontSize: "0.8rem" }}>● Paid</span>
                      )}
                    </td>
                    <td style={{ textAlign: "right", fontWeight: 600 }}>
                      {formatPaise(sale.grand_total_paise)}
                    </td>
                    <td style={{ textAlign: "center" }}>
                      <span
                        style={{
                          display: "inline-block",
                          padding: "0.2rem 0.6rem",
                          borderRadius: "12px",
                          fontSize: "0.8rem",
                          fontWeight: 600,
                          backgroundColor: statusBg,
                          color: statusColor,
                          textTransform: "capitalize",
                        }}
                      >
                        {sale.status}
                      </span>
                    </td>
                    <td style={{ textAlign: "right" }}>
                      <Link
                        href={`/sales/${sale.sale_id}`}
                        className="btn btn-secondary"
                        style={{ padding: "0.3rem 0.7rem", fontSize: "0.85rem" }}
                      >
                        {sale.status === "draft" ? "Edit Draft" : "View Bill"}
                      </Link>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
