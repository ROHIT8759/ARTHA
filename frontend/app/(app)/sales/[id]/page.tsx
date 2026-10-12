"use client";

import { useEffect, useState, useRef } from "react";
import { useParams, useRouter } from "next/navigation";
import Link from "next/link";
import { api, SaleView, ProductView, BusinessView, formatPaise } from "@/lib/api";
import LoadingState from "@/components/ui/LoadingState";
import ErrorState from "@/components/ui/ErrorState";
import A4Invoice from "@/components/invoice/A4Invoice";
import ThermalReceipt from "@/components/invoice/ThermalReceipt";

interface CartLine {
  sale_item_id?: string;
  product_id: string;
  product_name_snapshot: string;
  batch_snapshot?: string | null;
  hsn_code_snapshot?: string | null;
  gst_rate_bps: number;
  quantity_milli: number;
  unit_price_paise: number;
  discount_paise: number;
  available_stock_milli?: number;
}

export default function SaleDetailPage() {
  const params = useParams();
  const saleId = params.id as string;
  const router = useRouter();

  const [sale, setSale] = useState<SaleView | null>(null);
  const [business, setBusiness] = useState<BusinessView | null>(null);
  const [layoutFormat, setLayoutFormat] = useState<"a4" | "thermal">("a4");
  const [previewDraft, setPreviewDraft] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  // Cart & Draft editing state
  const [cart, setCart] = useState<CartLine[]>([]);
  const [customerName, setCustomerName] = useState("");
  const [customerPhone, setCustomerPhone] = useState("");
  const [customerGstin, setCustomerGstin] = useState("");
  const [paymentMethod, setPaymentMethod] = useState("cash");
  const [billDiscountPaise, setBillDiscountPaise] = useState(0);
  const [notes, setNotes] = useState("");

  // QR & Search state
  const [qrInput, setQrInput] = useState("");
  const [qrScanning, setQrScanning] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<ProductView[]>([]);
  const [isSearching, setIsSearching] = useState(false);

  // Submission state
  const [saving, setSaving] = useState(false);
  const [finalizing, setFinalizing] = useState(false);
  const [voiding, setVoiding] = useState(false);

  const qrInputRef = useRef<HTMLInputElement>(null);

  // Load sale data and business profile
  useEffect(() => {
    let mounted = true;
    const fetchData = async () => {
      try {
        setLoading(true);
        const [saleData, businessData] = await Promise.all([
          api.getSale(saleId),
          api.getBusiness().catch(() => null),
        ]);

        if (mounted) {
          setSale(saleData);
          if (businessData) {
            setBusiness(businessData);
            if (businessData.settings?.default_layout === "thermal") {
              setLayoutFormat("thermal");
            }
          }

          setCustomerName(saleData.customer_name || "");
          setCustomerPhone(saleData.customer_phone || "");
          setCustomerGstin(saleData.customer_gstin || "");
          setPaymentMethod(saleData.payment_method || "cash");
          setBillDiscountPaise(saleData.discount_paise || 0);
          setNotes(saleData.notes || "");

          if (saleData.status === "draft") {
            setCart(
              saleData.items.map((i) => ({
                sale_item_id: i.sale_item_id,
                product_id: i.product_id,
                product_name_snapshot: i.product_name_snapshot,
                batch_snapshot: i.batch_snapshot,
                hsn_code_snapshot: i.hsn_code_snapshot,
                gst_rate_bps: i.gst_rate_bps,
                quantity_milli: i.quantity_milli,
                unit_price_paise: i.unit_price_paise,
                discount_paise: i.discount_paise,
              }))
            );
          }
          setError(null);
        }
      } catch (err: unknown) {
        if (mounted) {
          setError(err instanceof Error ? err.message : "Failed to load sale");
        }
      } finally {
        if (mounted) {
          setLoading(false);
        }
      }
    };

    fetchData();
    return () => {
      mounted = false;
    };
  }, [saleId]);

  // Focus QR input automatically on draft load
  useEffect(() => {
    if (sale?.status === "draft" && qrInputRef.current) {
      qrInputRef.current.focus();
    }
  }, [sale?.status]);

  // Handle QR Scan
  const handleQrScan = async (e: React.FormEvent) => {
    e.preventDefault();
    const code = qrInput.trim();
    if (!code) return;

    setQrScanning(true);
    setActionError(null);

    try {
      const product = await api.getProductByQr(code);

      if (product.status !== "active") {
        throw new Error(`Product "${product.name}" is archived and cannot be sold.`);
      }

      addProductToCart(product);
      setQrInput("");
    } catch (err: unknown) {
      setActionError(err instanceof Error ? err.message : "QR lookup failed");
    } finally {
      setQrScanning(false);
      qrInputRef.current?.focus();
    }
  };

  // Product Search handler
  useEffect(() => {
    if (!searchQuery.trim()) {
      return;
    }

    const timer = setTimeout(async () => {
      setIsSearching(true);
      try {
        const results = await api.listProducts(searchQuery.trim());
        setSearchResults(results.filter((p) => p.status === "active"));
      } catch {
        setSearchResults([]);
      } finally {
        setIsSearching(false);
      }
    }, 250);

    return () => clearTimeout(timer);
  }, [searchQuery]);

  const addProductToCart = (product: ProductView) => {
    setActionError(null);
    setCart((prev) => {
      const existingIdx = prev.findIndex((i) => i.product_id === product.product_id);
      if (existingIdx >= 0) {
        // Increment quantity by 1 unit (1000 milli)
        const updated = [...prev];
        updated[existingIdx].quantity_milli += 1000;
        return updated;
      } else {
        return [
          ...prev,
          {
            product_id: product.product_id,
            product_name_snapshot: product.name,
            batch_snapshot: product.batch,
            hsn_code_snapshot: product.hsn_code,
            gst_rate_bps: product.gst_rate_bps,
            quantity_milli: 1000, // 1 unit
            unit_price_paise: product.price_paise,
            discount_paise: 0,
            available_stock_milli: product.stock_qty_milli,
          },
        ];
      }
    });
    setSearchQuery("");
    setSearchResults([]);
    qrInputRef.current?.focus();
  };

  const updateQuantity = (index: number, newQtyMilli: number) => {
    if (newQtyMilli <= 0) {
      removeCartItem(index);
      return;
    }
    setCart((prev) => {
      const updated = [...prev];
      updated[index].quantity_milli = newQtyMilli;
      return updated;
    });
  };

  const updateLineDiscount = (index: number, discountPaise: number) => {
    setCart((prev) => {
      const updated = [...prev];
      updated[index].discount_paise = Math.max(0, discountPaise);
      return updated;
    });
  };

  const removeCartItem = (index: number) => {
    setCart((prev) => prev.filter((_, i) => i !== index));
  };

  // Live client-side calculation estimates strictly using integer paise arithmetic matching backend
  const calculatedSubtotal = cart.reduce((acc, item) => {
    const lineSub = Math.floor((item.quantity_milli * item.unit_price_paise + 500) / 1000);
    return acc + lineSub;
  }, 0);

  const calculatedLineDiscounts = cart.reduce((acc, item) => acc + item.discount_paise, 0);
  const totalCalculatedDiscount = calculatedLineDiscounts + billDiscountPaise;

  const calculatedTax = cart.reduce((acc, item) => {
    const lineSub = Math.floor((item.quantity_milli * item.unit_price_paise + 500) / 1000);
    const taxable = Math.max(0, lineSub - item.discount_paise);
    const lineTax = Math.floor((taxable * item.gst_rate_bps + 5000) / 10000);
    return acc + lineTax;
  }, 0);

  const calculatedGrandTotal = Math.max(0, calculatedSubtotal - totalCalculatedDiscount + calculatedTax);

  // Save draft
  const handleSaveDraft = async () => {
    setSaving(true);
    setActionError(null);
    try {
      const updated = await api.updateSale(saleId, {
        customer_name: customerName,
        customer_phone: customerPhone,
        customer_gstin: customerGstin,
        payment_method: paymentMethod,
        discount_paise: billDiscountPaise,
        notes: notes,
        items: cart.map((i) => ({
          sale_item_id: i.sale_item_id,
          product_id: i.product_id,
          quantity_milli: i.quantity_milli,
          unit_price_paise: i.unit_price_paise,
          discount_paise: i.discount_paise,
        })),
      });
      setSale(updated);
      alert("Draft autosaved successfully!");
    } catch (err: unknown) {
      setActionError(err instanceof Error ? err.message : "Failed to save draft");
    } finally {
      setSaving(false);
    }
  };

  // Finalize sale
  const handleFinalize = async () => {
    if (cart.length === 0) {
      setActionError("Cart is empty! Add products before billing.");
      return;
    }

    if (!confirm(`Finalize sale for ${formatPaise(calculatedGrandTotal)} via ${paymentMethod.toUpperCase()}?`)) {
      return;
    }

    setFinalizing(true);
    setActionError(null);

    try {
      // 1. Sync latest cart state to draft first
      await api.updateSale(saleId, {
        customer_name: customerName,
        customer_phone: customerPhone,
        customer_gstin: customerGstin,
        payment_method: paymentMethod,
        discount_paise: billDiscountPaise,
        notes: notes,
        items: cart.map((i) => ({
          sale_item_id: i.sale_item_id,
          product_id: i.product_id,
          quantity_milli: i.quantity_milli,
          unit_price_paise: i.unit_price_paise,
          discount_paise: i.discount_paise,
        })),
      });

      // 2. Finalize sale atomically
      const finalized = await api.finalizeSale(saleId, {
        payment_method: paymentMethod,
        amount_paise: calculatedGrandTotal,
      });

      setSale(finalized);
    } catch (err: unknown) {
      setActionError(err instanceof Error ? err.message : "Failed to finalize sale");
    } finally {
      setFinalizing(false);
    }
  };

  // Void sale
  const handleVoid = async () => {
    const isCompleted = sale?.status === "completed";
    const promptMsg = isCompleted
      ? "VOID this completed invoice? This will restore inventory stock to the ledger. Note: Payment collection remains on record; no automated payment refund is issued (any customer refund must be handled manually)."
      : "Cancel and void this draft sale?";

    if (!confirm(promptMsg)) return;

    const reason = prompt("Reason for voiding:", "Customer request / mistake");
    if (reason === null) return;

    setVoiding(true);
    setActionError(null);

    try {
      const voided = await api.voidSale(saleId, reason || "Voided by user");
      setSale(voided);
    } catch (err: unknown) {
      setActionError(err instanceof Error ? err.message : "Failed to void sale");
    } finally {
      setVoiding(false);
    }
  };

  if (loading && !sale) return <LoadingState />;
  if (error && !sale) return <ErrorState message={error} onRetry={() => window.location.reload()} />;
  if (!sale) return null;

  // ==========================================
  // DOCUMENT VIEW (A4 Tax Invoice & Thermal POS)
  // ==========================================
  if (sale.status === "completed" || sale.status === "void" || previewDraft) {
    const isVoid = sale.status === "void";
    const isDraft = sale.status === "draft";

    // Reactive sale snapshot for proforma draft preview
    const effectiveSale: SaleView =
      isDraft && previewDraft
        ? {
            ...sale,
            customer_name: customerName,
            customer_phone: customerPhone,
            customer_gstin: customerGstin,
            payment_method: paymentMethod,
            subtotal_paise: calculatedSubtotal,
            discount_paise: totalCalculatedDiscount,
            tax_total_paise: calculatedTax,
            grand_total_paise: calculatedGrandTotal,
            items: cart.map((c, i) => {
              const lineSub = Math.floor((c.quantity_milli * c.unit_price_paise + 500) / 1000);
              const taxable = Math.max(0, lineSub - c.discount_paise);
              const lineTax = Math.floor((taxable * c.gst_rate_bps + 5000) / 10000);
              return {
                sale_item_id: c.sale_item_id || `draft-item-${i}`,
                sale_id: sale.sale_id,
                product_id: c.product_id,
                product_name_snapshot: c.product_name_snapshot,
                batch_snapshot: c.batch_snapshot || null,
                hsn_code_snapshot: c.hsn_code_snapshot || null,
                gst_rate_bps: c.gst_rate_bps,
                quantity_milli: c.quantity_milli,
                unit_price_paise: c.unit_price_paise,
                discount_paise: c.discount_paise,
                tax_paise: lineTax,
                line_total_paise: taxable + lineTax,
              };
            }),
          }
        : sale;

    return (
      <div style={{ maxWidth: layoutFormat === "thermal" ? 420 : 880, margin: "0 auto", paddingBottom: "3rem" }}>
        {/* Screen-only Action Toolbar */}
        <div
          className="no-print"
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            flexWrap: "wrap",
            gap: "1rem",
            marginBottom: "1.5rem",
            padding: "1rem",
            backgroundColor: "var(--background)",
            borderRadius: "8px",
            border: "1px solid var(--border)",
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: "0.75rem", flexWrap: "wrap" }}>
            {isDraft ? (
              <button
                onClick={() => setPreviewDraft(false)}
                className="btn btn-secondary"
                style={{ fontWeight: 600 }}
              >
                &larr; Back to Checkout
              </button>
            ) : (
              <Link href="/sales" className="btn btn-secondary" style={{ fontWeight: 600 }}>
                &larr; Back to Sales
              </Link>
            )}

            {/* Layout switch controls */}
            <div style={{ display: "inline-flex", border: "1px solid var(--border)", borderRadius: "6px", overflow: "hidden" }}>
              <button
                type="button"
                onClick={() => setLayoutFormat("a4")}
                style={{
                  padding: "0.45rem 0.85rem",
                  fontSize: "0.85rem",
                  border: "none",
                  cursor: "pointer",
                  fontWeight: 600,
                  backgroundColor: layoutFormat === "a4" ? "var(--accent)" : "transparent",
                  color: layoutFormat === "a4" ? "#fff" : "var(--foreground)",
                }}
              >
                📄 A4 Tax Invoice
              </button>
              <button
                type="button"
                onClick={() => setLayoutFormat("thermal")}
                style={{
                  padding: "0.45rem 0.85rem",
                  fontSize: "0.85rem",
                  border: "none",
                  cursor: "pointer",
                  fontWeight: 600,
                  backgroundColor: layoutFormat === "thermal" ? "var(--accent)" : "transparent",
                  color: layoutFormat === "thermal" ? "#fff" : "var(--foreground)",
                }}
              >
                🧾 Thermal Receipt (80mm)
              </button>
            </div>
          </div>

          <div style={{ display: "flex", gap: "0.75rem", flexWrap: "wrap" }}>
            <button
              onClick={() => window.print()}
              className="btn"
              style={{ display: "flex", alignItems: "center", gap: "0.5rem" }}
            >
              🖨️ Print Document
            </button>

            {isDraft && (
              <button
                onClick={handleFinalize}
                disabled={finalizing || cart.length === 0}
                className="btn"
                style={{ background: "#2563eb", color: "#fff" }}
              >
                {finalizing ? "Finalizing..." : `⚡ Finalize & Bill ${formatPaise(calculatedGrandTotal)}`}
              </button>
            )}

            {!isDraft && (
              <button
                onClick={() => router.push("/sales/new")}
                className="btn"
                style={{ background: "#2563eb", color: "#fff" }}
              >
                ⚡ New Sale
              </button>
            )}

            {!isVoid && !isDraft && (
              <button
                onClick={handleVoid}
                disabled={voiding}
                className="btn btn-secondary"
                style={{ color: "var(--danger)", borderColor: "var(--danger)" }}
              >
                {voiding ? "Voiding..." : "Void Sale"}
              </button>
            )}
          </div>
        </div>

        {actionError && (
          <div className="card no-print" style={{ marginBottom: "1rem", borderColor: "var(--danger)", backgroundColor: "rgba(179, 38, 30, 0.05)" }}>
            <p style={{ color: "var(--danger)", margin: 0, fontWeight: 600 }}>{actionError}</p>
          </div>
        )}

        {/* Render Layout */}
        {layoutFormat === "a4" ? (
          <A4Invoice sale={effectiveSale} business={business} />
        ) : (
          <ThermalReceipt sale={effectiveSale} business={business} />
        )}
      </div>
    );
  }

  // ==========================================
  // DRAFT / POS TERMINAL MODE
  // ==========================================
  return (
    <div>
      {/* Top Header */}
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "1.5rem" }}>
        <div>
          <Link href="/sales" className="btn btn-secondary" style={{ marginBottom: "0.5rem", display: "inline-block" }}>
            &larr; Back to Sales
          </Link>
          <h1 style={{ margin: 0 }}>POS Checkout Terminal</h1>
          <p className="muted" style={{ margin: "0.2rem 0 0" }}>Fast barcode/QR scanner or manual search</p>
        </div>
        <div style={{ display: "flex", gap: "0.75rem", flexWrap: "wrap" }}>
          <button
            type="button"
            onClick={() => setPreviewDraft(true)}
            className="btn btn-secondary"
            style={{ display: "flex", alignItems: "center", gap: "0.4rem" }}
          >
            👁️ Preview / Print Proforma
          </button>
          <button
            onClick={handleSaveDraft}
            disabled={saving || finalizing}
            className="btn btn-secondary"
          >
            {saving ? "Saving..." : "💾 Save Draft"}
          </button>
          <button
            onClick={handleVoid}
            disabled={voiding || finalizing}
            className="btn btn-secondary"
            style={{ color: "var(--danger)", borderColor: "var(--danger)" }}
          >
            Discard
          </button>
        </div>
      </div>

      {actionError && (
        <div className="card" style={{ marginBottom: "1rem", borderColor: "var(--danger)", backgroundColor: "rgba(179, 38, 30, 0.05)" }}>
          <p style={{ color: "var(--danger)", margin: 0, fontWeight: 600 }}>{actionError}</p>
        </div>
      )}

      <div style={{ display: "grid", gridTemplateColumns: "1fr 340px", gap: "1.5rem", alignItems: "start" }}>
        {/* Left Column: QR Scan, Product Search & Cart */}
        <div>
          {/* Quick Scanner & Search Box */}
          <div className="card" style={{ marginBottom: "1rem", padding: "1.25rem" }}>
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "1rem" }}>
              {/* QR Scanner input */}
              <form onSubmit={handleQrScan}>
                <label style={{ display: "block", fontSize: "0.85rem", fontWeight: 600, marginBottom: "0.35rem" }}>
                  📷 Scan Barcode / QR Code
                </label>
                <div style={{ display: "flex", gap: "0.5rem" }}>
                  <input
                    ref={qrInputRef}
                    type="text"
                    placeholder="Scan QR or press Enter..."
                    value={qrInput}
                    onChange={(e) => setQrInput(e.target.value)}
                    disabled={qrScanning}
                    style={{ flex: 1, padding: "0.55rem 0.75rem", borderRadius: "6px", border: "2px solid var(--accent)" }}
                  />
                  <button type="submit" className="btn" disabled={qrScanning || !qrInput.trim()}>
                    {qrScanning ? "..." : "Add"}
                  </button>
                </div>
              </form>

              {/* Product Search fallback */}
              <div style={{ position: "relative" }}>
                <label style={{ display: "block", fontSize: "0.85rem", fontWeight: 600, marginBottom: "0.35rem" }}>
                  🔍 Search Catalog Fallback
                </label>
                <input
                  type="text"
                  placeholder="Type product name or batch..."
                  value={searchQuery}
                  onChange={(e) => {
                    const q = e.target.value;
                    setSearchQuery(q);
                    if (!q.trim()) setSearchResults([]);
                  }}
                  style={{ width: "100%", padding: "0.55rem 0.75rem", borderRadius: "6px", border: "1px solid var(--border)" }}
                />

                {/* Dropdown search results */}
                {searchResults.length > 0 && (
                  <div
                    className="card"
                    style={{
                      position: "absolute",
                      top: "100%",
                      left: 0,
                      right: 0,
                      zIndex: 100,
                      backgroundColor: "var(--background)",
                      maxHeight: 250,
                      overflowY: "auto",
                      padding: "0.5rem",
                      marginTop: "0.25rem",
                      boxShadow: "0 4px 12px rgba(0,0,0,0.15)",
                    }}
                  >
                    {searchResults.map((p) => (
                      <div
                        key={p.product_id}
                        onClick={() => addProductToCart(p)}
                        style={{
                          padding: "0.5rem 0.75rem",
                          borderBottom: "1px solid var(--border)",
                          cursor: "pointer",
                          display: "flex",
                          justifyContent: "space-between",
                          alignItems: "center",
                        }}
                      >
                        <div>
                          <div style={{ fontWeight: 600 }}>{p.name}</div>
                          <div className="muted" style={{ fontSize: "0.8rem" }}>
                            Stock: {(p.stock_qty_milli / 1000).toFixed(0)} {p.unit} | {p.qr_code}
                          </div>
                        </div>
                        <div style={{ fontWeight: 600, color: "var(--accent)" }}>
                          {formatPaise(p.price_paise)}
                        </div>
                      </div>
                    ))}
                  </div>
                )}
                {isSearching && (
                  <div className="muted" style={{ position: "absolute", right: 10, top: 32, fontSize: "0.8rem" }}>
                    Searching...
                  </div>
                )}
              </div>
            </div>
          </div>

          {/* Cart Table */}
          <div className="card" style={{ padding: 0, overflowX: "auto" }}>
            <div style={{ padding: "1rem", borderBottom: "1px solid var(--border)", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
              <h3 style={{ margin: 0 }}>Cart ({cart.length} {cart.length === 1 ? "item" : "items"})</h3>
              {cart.length > 0 && (
                <button
                  onClick={() => setCart([])}
                  className="btn btn-secondary"
                  style={{ fontSize: "0.8rem", padding: "0.2rem 0.5rem", color: "var(--danger)" }}
                >
                  Clear Cart
                </button>
              )}
            </div>

            {cart.length === 0 ? (
              <div style={{ padding: "3rem 1rem", textAlign: "center" }}>
                <p className="muted" style={{ fontSize: "1.1rem", margin: 0 }}>
                  🛒 Cart is empty. Scan a product QR code or search to add items.
                </p>
              </div>
            ) : (
              <table style={{ margin: 0 }}>
                <thead>
                  <tr style={{ background: "rgba(0,0,0,0.02)" }}>
                    <th>Product</th>
                    <th style={{ width: 140, textAlign: "center" }}>Quantity</th>
                    <th style={{ textAlign: "right" }}>Unit Price</th>
                    <th style={{ width: 100, textAlign: "right" }}>Disc (₹)</th>
                    <th style={{ textAlign: "right" }}>GST</th>
                    <th style={{ textAlign: "right" }}>Total</th>
                    <th style={{ width: 40 }}></th>
                  </tr>
                </thead>
                <tbody>
                  {cart.map((item, idx) => {
                    const units = item.quantity_milli / 1000;
                    const lineSub = Math.floor((item.quantity_milli * item.unit_price_paise + 500) / 1000);
                    const taxable = Math.max(0, lineSub - item.discount_paise);
                    const lineTax = Math.floor((taxable * item.gst_rate_bps + 5000) / 10000);
                    const lineTotal = taxable + lineTax;

                    return (
                      <tr key={idx}>
                        <td>
                          <div style={{ fontWeight: 600 }}>{item.product_name_snapshot}</div>
                          <div className="muted" style={{ fontSize: "0.8rem" }}>
                            {item.hsn_code_snapshot ? `HSN: ${item.hsn_code_snapshot}` : ""}
                            {item.batch_snapshot ? ` | Batch: ${item.batch_snapshot}` : ""}
                          </div>
                        </td>
                        <td style={{ textAlign: "center" }}>
                          <div style={{ display: "inline-flex", alignItems: "center", border: "1px solid var(--border)", borderRadius: "4px" }}>
                            <button
                              type="button"
                              onClick={() => updateQuantity(idx, item.quantity_milli - 1000)}
                              style={{ border: "none", background: "transparent", padding: "0.25rem 0.5rem", cursor: "pointer", fontWeight: 700 }}
                            >
                              -
                            </button>
                            <input
                              type="number"
                              min="1"
                              step="1"
                              value={units}
                              onChange={(e) => {
                                const val = parseFloat(e.target.value) || 0;
                                updateQuantity(idx, Math.round(val * 1000));
                              }}
                              style={{ width: "45px", textAlign: "center", border: "none", background: "transparent", fontWeight: 600 }}
                            />
                            <button
                              type="button"
                              onClick={() => updateQuantity(idx, item.quantity_milli + 1000)}
                              style={{ border: "none", background: "transparent", padding: "0.25rem 0.5rem", cursor: "pointer", fontWeight: 700 }}
                            >
                              +
                            </button>
                          </div>
                        </td>
                        <td style={{ textAlign: "right" }}>{formatPaise(item.unit_price_paise)}</td>
                        <td style={{ textAlign: "right" }}>
                          <input
                            type="number"
                            min="0"
                            step="1"
                            placeholder="0"
                            value={item.discount_paise ? item.discount_paise / 100 : ""}
                            onChange={(e) => {
                              const val = parseFloat(e.target.value) || 0;
                              updateLineDiscount(idx, Math.round(val * 100));
                            }}
                            style={{ width: "70px", textAlign: "right", padding: "0.2rem 0.4rem", borderRadius: "4px", border: "1px solid var(--border)" }}
                          />
                        </td>
                        <td style={{ textAlign: "right" }}>{(item.gst_rate_bps / 100).toFixed(0)}%</td>
                        <td style={{ textAlign: "right", fontWeight: 600 }}>{formatPaise(lineTotal)}</td>
                        <td style={{ textAlign: "center" }}>
                          <button
                            type="button"
                            onClick={() => removeCartItem(idx)}
                            style={{ background: "transparent", border: "none", color: "var(--danger)", cursor: "pointer", fontWeight: 700 }}
                            title="Remove item"
                          >
                            ✕
                          </button>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            )}
          </div>
        </div>

        {/* Right Column: Customer Info & Checkout Billing Summary */}
        <div>
          {/* Customer info card */}
          <div className="card" style={{ marginBottom: "1rem", padding: "1.25rem" }}>
            <h3 style={{ margin: "0 0 1rem", fontSize: "1.1rem" }}>👤 Customer Details</h3>
            <div className="field">
              <label style={{ fontSize: "0.85rem" }}>Customer Name</label>
              <input
                type="text"
                placeholder="Walk-in Customer"
                value={customerName}
                onChange={(e) => setCustomerName(e.target.value)}
              />
            </div>
            <div className="field">
              <label style={{ fontSize: "0.85rem" }}>Phone Number</label>
              <input
                type="tel"
                placeholder="10-digit mobile number"
                value={customerPhone}
                onChange={(e) => setCustomerPhone(e.target.value)}
              />
            </div>
            <div className="field">
              <label style={{ fontSize: "0.85rem" }}>GSTIN (Optional)</label>
              <input
                type="text"
                placeholder="GST Identification Number"
                value={customerGstin}
                onChange={(e) => setCustomerGstin(e.target.value)}
              />
            </div>
          </div>

          {/* Billing & Payment card */}
          <div className="card" style={{ padding: "1.25rem", border: "2px solid var(--accent)" }}>
            <h3 style={{ margin: "0 0 1rem", fontSize: "1.1rem" }}>💳 Payment & Bill Total</h3>

            {/* Payment Method Selector */}
            <div className="field" style={{ marginBottom: "1.25rem" }}>
              <label style={{ fontSize: "0.85rem", fontWeight: 600 }}>Payment Method</label>
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "0.5rem" }}>
                {(["cash", "upi", "card", "other"] as const).map((method) => (
                  <button
                    key={method}
                    type="button"
                    onClick={() => setPaymentMethod(method)}
                    className="btn btn-secondary"
                    style={{
                      textTransform: "uppercase",
                      fontSize: "0.85rem",
                      fontWeight: 600,
                      borderColor: paymentMethod === method ? "var(--accent)" : "var(--border)",
                      backgroundColor: paymentMethod === method ? "rgba(15,107,76,0.15)" : "transparent",
                      color: paymentMethod === method ? "var(--accent)" : "inherit",
                    }}
                  >
                    {method}
                  </button>
                ))}
              </div>
            </div>

            {/* Bill discount */}
            <div className="field" style={{ marginBottom: "1rem" }}>
              <label style={{ fontSize: "0.85rem" }}>Bill-Level Discount (₹)</label>
              <input
                type="number"
                min="0"
                step="1"
                placeholder="0"
                value={billDiscountPaise ? billDiscountPaise / 100 : ""}
                onChange={(e) => {
                  const val = parseFloat(e.target.value) || 0;
                  setBillDiscountPaise(Math.round(val * 100));
                }}
              />
            </div>

            {/* Financial breakdown */}
            <div style={{ borderTop: "1px solid var(--border)", paddingTop: "0.75rem", marginBottom: "1rem" }}>
              <div style={{ display: "flex", justifyContent: "space-between", padding: "0.25rem 0", fontSize: "0.95rem" }}>
                <span className="muted">Subtotal:</span>
                <span>{formatPaise(calculatedSubtotal)}</span>
              </div>
              {totalCalculatedDiscount > 0 && (
                <div style={{ display: "flex", justifyContent: "space-between", padding: "0.25rem 0", fontSize: "0.95rem", color: "var(--accent)" }}>
                  <span>Total Discount:</span>
                  <span>- {formatPaise(totalCalculatedDiscount)}</span>
                </div>
              )}
              <div style={{ display: "flex", justifyContent: "space-between", padding: "0.25rem 0", fontSize: "0.95rem" }}>
                <span className="muted">GST Tax:</span>
                <span>{formatPaise(calculatedTax)}</span>
              </div>
              <div
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  padding: "0.75rem 0 0",
                  marginTop: "0.5rem",
                  borderTop: "2px solid var(--border)",
                  fontSize: "1.3rem",
                  fontWeight: 800,
                }}
              >
                <span>Grand Total:</span>
                <span style={{ color: "var(--accent)" }}>{formatPaise(calculatedGrandTotal)}</span>
              </div>
            </div>

            {/* Primary Finalize Button */}
            <button
              onClick={handleFinalize}
              disabled={finalizing || cart.length === 0}
              className="btn"
              style={{
                width: "100%",
                padding: "0.85rem",
                fontSize: "1.1rem",
                textAlign: "center",
                display: "block",
              }}
            >
              {finalizing ? "Processing Bill..." : `⚡ Finalize & Bill ${formatPaise(calculatedGrandTotal)}`}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
