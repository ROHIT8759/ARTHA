"use client";

import React from "react";
import { SaleView, BusinessView, formatPaise, amountToWords } from "@/lib/api";

interface A4InvoiceProps {
  sale: SaleView;
  business: BusinessView | null;
}


export default function A4Invoice({ sale, business }: A4InvoiceProps) {
  const isVoid = sale.status === "void";
  const isDraft = sale.status === "draft";
  const settings = business?.settings;

  const showHsn = settings?.show_hsn !== false;
  const showLogo = settings?.show_logo !== false && !!settings?.logo_data_url;
  const showTaxBreakdown = settings?.show_tax_breakdown !== false;

  const businessName = settings?.registered_name?.trim() || business?.name?.trim() || "ARTHA RETAIL SYSTEM";
  const addressParts = [
    settings?.address,
    settings?.city,
    settings?.state ? `${settings.state}${settings?.pincode ? ` - ${settings.pincode}` : ""}` : settings?.pincode,
    settings?.country,
  ].filter(Boolean);
  const formattedAddress = addressParts.join(", ");

  // Authoritative Tax Slab Breakdown from persisted line records
  // Derivation identity: line_total_paise = taxable_line + tax_paise
  // Therefore: taxable_line = line_total_paise - tax_paise (exact integer paise, no float rounding errors)
  const taxSlabs: { [key: number]: { rateBps: number; taxablePaise: number; taxPaise: number } } = {};
  for (const item of sale.items) {
    const lineTax = item.tax_paise;
    const taxable = Math.max(0, item.line_total_paise - lineTax);

    if (!taxSlabs[item.gst_rate_bps]) {
      taxSlabs[item.gst_rate_bps] = {
        rateBps: item.gst_rate_bps,
        taxablePaise: 0,
        taxPaise: 0,
      };
    }
    taxSlabs[item.gst_rate_bps].taxablePaise += taxable;
    taxSlabs[item.gst_rate_bps].taxPaise += lineTax;
  }

  const slabList = Object.values(taxSlabs).sort((a, b) => a.rateBps - b.rateBps);

  // Document Title & Badges
  let docTitle = "TAX INVOICE";
  let badgeLabel = "PAID";
  let badgeColor = "var(--accent)";
  let badgeBg = "rgba(15, 107, 76, 0.15)";

  if (isDraft) {
    docTitle = "DRAFT ESTIMATE / PROFORMA";
    badgeLabel = "DRAFT - NOT FINAL";
    badgeColor = "#b45309";
    badgeBg = "rgba(245, 158, 11, 0.15)";
  } else if (isVoid) {
    docTitle = "VOID INVOICE";
    badgeLabel = "VOID / CANCELLED";
    badgeColor = "var(--danger)";
    badgeBg = "rgba(179, 38, 30, 0.15)";
  }

  const invoiceDate = sale.finalized_at
    ? new Date(sale.finalized_at).toLocaleString("en-IN", {
        dateStyle: "medium",
        timeStyle: "short",
      })
    : new Date(sale.created_at).toLocaleString("en-IN", {
        dateStyle: "medium",
        timeStyle: "short",
      });

  return (
    <div
      className="card a4-invoice-container"
      style={{
        position: "relative",
        backgroundColor: "#ffffff",
        color: "#111827",
        padding: "2.5rem",
        boxShadow: "0 4px 14px rgba(0,0,0,0.06)",
        borderRadius: "8px",
        overflow: "hidden",
      }}
    >
      {/* Watermark */}
      {isDraft && (
        <div
          aria-hidden="true"
          style={{
            position: "absolute",
            top: "45%",
            left: "50%",
            transform: "translate(-50%, -50%) rotate(-30deg)",
            fontSize: "6.5rem",
            fontWeight: 900,
            color: "rgba(217, 119, 6, 0.10)",
            userSelect: "none",
            pointerEvents: "none",
            letterSpacing: "0.25em",
            zIndex: 1,
            whiteSpace: "nowrap",
          }}
        >
          DRAFT
        </div>
      )}

      {isVoid && (
        <div
          aria-hidden="true"
          style={{
            position: "absolute",
            top: "45%",
            left: "50%",
            transform: "translate(-50%, -50%) rotate(-30deg)",
            fontSize: "6.5rem",
            fontWeight: 900,
            color: "rgba(220, 38, 38, 0.12)",
            userSelect: "none",
            pointerEvents: "none",
            letterSpacing: "0.25em",
            zIndex: 1,
            whiteSpace: "nowrap",
          }}
        >
          VOID
        </div>
      )}

      {/* Top Warning Banner for Draft or Void */}
      {isDraft && (
        <div
          className="avoid-break"
          style={{
            backgroundColor: "#fffbeb",
            border: "1px solid #fcd34d",
            color: "#92400e",
            padding: "0.75rem 1rem",
            borderRadius: "6px",
            marginBottom: "1.5rem",
            fontSize: "0.9rem",
            fontWeight: 600,
            textAlign: "center",
          }}
        >
          ⚠️ DRAFT DOCUMENT: This is an unfinalized estimate / proforma. No official tax invoice has been generated, and inventory has not been deducted.
        </div>
      )}

      {isVoid && (
        <div
          className="avoid-break"
          style={{
            backgroundColor: "#fef2f2",
            border: "1px solid #fecaca",
            color: "#991b1b",
            padding: "0.75rem 1rem",
            borderRadius: "6px",
            marginBottom: "1.5rem",
            fontSize: "0.9rem",
            fontWeight: 600,
            textAlign: "center",
          }}
        >
          ⚠️ VOID INVOICE: This invoice was cancelled. Associated inventory deductions have been reversed in the ledger. Note: Payment collection remains on record; no automated payment refund has been processed.
        </div>
      )}

      {/* Header: Business Identity & Document Info */}
      <div
        className="avoid-break"
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "flex-start",
          borderBottom: "2px solid #e5e7eb",
          paddingBottom: "1.5rem",
          marginBottom: "1.5rem",
          position: "relative",
          zIndex: 2,
        }}
      >
        <div style={{ maxWidth: "60%" }}>
          {showLogo && settings?.logo_data_url && (
            <div style={{ marginBottom: "0.75rem" }}>
              {/* eslint-disable-next-line @next/next/no-img-element */}
              <img
                src={settings.logo_data_url}
                alt={businessName}
                style={{
                  maxHeight: "65px",
                  maxWidth: "200px",
                  objectFit: "contain",
                  display: "block",
                }}
              />
            </div>
          )}
          <h1 style={{ margin: "0 0 0.35rem", fontSize: "1.6rem", fontWeight: 800, color: "#111827" }}>
            {businessName}
          </h1>
          {formattedAddress && (
            <p style={{ margin: "0 0 0.25rem", fontSize: "0.85rem", color: "#4b5563", lineHeight: 1.4 }}>
              {formattedAddress}
            </p>
          )}
          <div style={{ fontSize: "0.85rem", color: "#4b5563", display: "flex", flexWrap: "wrap", gap: "1rem" }}>
            {settings?.phone && <span>Phone: <strong>{settings.phone}</strong></span>}
            {settings?.email && <span>Email: <strong>{settings.email}</strong></span>}
          </div>
          {settings?.gstin && (
            <div style={{ marginTop: "0.35rem", fontSize: "0.9rem", color: "#111827" }}>
              GSTIN / UIN: <strong style={{ fontFamily: "monospace" }}>{settings.gstin}</strong>
            </div>
          )}
        </div>

        <div style={{ textAlign: "right", minWidth: "220px" }}>
          <h2 style={{ margin: "0 0 0.25rem", fontSize: "1.4rem", letterSpacing: "0.05em", color: "#1f2937" }}>
            {docTitle}
          </h2>
          <div style={{ fontSize: "1.2rem", fontWeight: 700, fontFamily: "monospace", color: "#111827" }}>
            {sale.invoice_no || "DRAFT"}
          </div>
          <div style={{ fontSize: "0.85rem", color: "#4b5563", marginTop: "0.35rem" }}>
            Date: {invoiceDate}
          </div>
          <div style={{ marginTop: "0.75rem" }}>
            <span
              style={{
                display: "inline-block",
                padding: "0.25rem 0.75rem",
                borderRadius: "4px",
                fontSize: "0.8rem",
                fontWeight: 700,
                textTransform: "uppercase",
                backgroundColor: badgeBg,
                color: badgeColor,
                letterSpacing: "0.05em",
              }}
            >
              {badgeLabel}
            </span>
          </div>
        </div>
      </div>

      {/* Customer & Billing Details */}
      <div
        className="avoid-break"
        style={{
          display: "grid",
          gridTemplateColumns: "1.2fr 1fr",
          gap: "1.5rem",
          marginBottom: "1.5rem",
          paddingBottom: "1.5rem",
          borderBottom: "1px solid #e5e7eb",
          position: "relative",
          zIndex: 2,
        }}
      >
        <div>
          <div style={{ fontSize: "0.75rem", textTransform: "uppercase", fontWeight: 700, color: "#6b7280", letterSpacing: "0.05em", marginBottom: "0.35rem" }}>
            Billed To / Customer
          </div>
          <div style={{ fontSize: "1.05rem", fontWeight: 700, color: "#111827" }}>
            {sale.customer_name?.trim() || "Walk-in Customer"}
          </div>
          {sale.customer_phone && (
            <div style={{ fontSize: "0.85rem", color: "#4b5563", marginTop: "0.2rem" }}>
              Mobile: {sale.customer_phone}
            </div>
          )}
          {sale.customer_email && (
            <div style={{ fontSize: "0.85rem", color: "#4b5563", marginTop: "0.2rem" }}>
              Email: {sale.customer_email}
            </div>
          )}
          {sale.customer_address && (
            <div style={{ fontSize: "0.85rem", color: "#4b5563", marginTop: "0.2rem" }}>
              Address: {sale.customer_address}
            </div>
          )}
          {sale.customer_gstin && (
            <div style={{ fontSize: "0.85rem", color: "#111827", marginTop: "0.35rem" }}>
              Customer GSTIN: <strong style={{ fontFamily: "monospace" }}>{sale.customer_gstin}</strong>
            </div>
          )}
        </div>

        <div style={{ textAlign: "right" }}>
          <div style={{ fontSize: "0.75rem", textTransform: "uppercase", fontWeight: 700, color: "#6b7280", letterSpacing: "0.05em", marginBottom: "0.35rem" }}>
            Payment & Billed By
          </div>
          <div style={{ fontSize: "0.9rem", color: "#111827" }}>
            Payment Mode: <strong style={{ textTransform: "uppercase" }}>{sale.payment_method}</strong>
          </div>
          <div style={{ fontSize: "0.9rem", color: "#111827", marginTop: "0.2rem" }}>
            Payment Status:{" "}
            <strong style={{ textTransform: "uppercase" }}>
              {isVoid && sale.payment_status === "paid" ? "PAID (UNREFUNDED)" : sale.payment_status}
            </strong>
          </div>
          {sale.finalized_by && (
            <div style={{ fontSize: "0.85rem", color: "#6b7280", marginTop: "0.2rem" }}>
              Cashier / Staff: {sale.finalized_by}
            </div>
          )}
          {sale.payments.length > 0 && (
            <div style={{ fontSize: "0.8rem", color: "#6b7280", marginTop: "0.25rem", fontFamily: "monospace" }}>
              Txn Ref: {sale.payments[0].payment_id.slice(0, 8)}...
            </div>
          )}
        </div>
      </div>

      {/* Items Table */}
      <div style={{ position: "relative", zIndex: 2, marginBottom: "1.5rem" }}>
        <table style={{ width: "100%", borderCollapse: "collapse" }}>
          <thead>
            <tr style={{ borderBottom: "2px solid #e5e7eb", background: "#f9fafb" }}>
              <th style={{ padding: "0.6rem 0.5rem", width: "35px", textAlign: "left", fontSize: "0.8rem", color: "#4b5563" }}>#</th>
              <th style={{ padding: "0.6rem 0.5rem", textAlign: "left", fontSize: "0.8rem", color: "#4b5563" }}>Item Description</th>
              {showHsn && <th style={{ padding: "0.6rem 0.5rem", width: "90px", textAlign: "left", fontSize: "0.8rem", color: "#4b5563" }}>HSN</th>}
              <th style={{ padding: "0.6rem 0.5rem", width: "65px", textAlign: "right", fontSize: "0.8rem", color: "#4b5563" }}>Qty</th>
              <th style={{ padding: "0.6rem 0.5rem", width: "95px", textAlign: "right", fontSize: "0.8rem", color: "#4b5563" }}>Rate</th>
              <th style={{ padding: "0.6rem 0.5rem", width: "80px", textAlign: "right", fontSize: "0.8rem", color: "#4b5563" }}>Disc</th>
              <th style={{ padding: "0.6rem 0.5rem", width: "70px", textAlign: "right", fontSize: "0.8rem", color: "#4b5563" }}>GST</th>
              <th style={{ padding: "0.6rem 0.5rem", width: "105px", textAlign: "right", fontSize: "0.8rem", color: "#4b5563" }}>Amount</th>
            </tr>
          </thead>
          <tbody>
            {sale.items.map((item, idx) => {
              const qtyFormatted = (item.quantity_milli / 1000).toFixed(
                item.quantity_milli % 1000 === 0 ? 0 : 2
              );

              return (
                <tr
                  key={item.sale_item_id || idx}
                  className="avoid-break"
                  style={{ borderBottom: "1px solid #f3f4f6" }}
                >
                  <td style={{ padding: "0.65rem 0.5rem", color: "#6b7280", fontSize: "0.85rem" }}>
                    {idx + 1}
                  </td>
                  <td style={{ padding: "0.65rem 0.5rem" }}>
                    <div style={{ fontWeight: 600, color: "#111827", fontSize: "0.9rem" }}>
                      {item.product_name_snapshot}
                    </div>
                    {item.batch_snapshot && (
                      <div style={{ fontSize: "0.75rem", color: "#6b7280" }}>
                        Batch: {item.batch_snapshot}
                      </div>
                    )}
                  </td>
                  {showHsn && (
                    <td style={{ padding: "0.65rem 0.5rem", fontSize: "0.85rem", color: "#4b5563", fontFamily: "monospace" }}>
                      {item.hsn_code_snapshot || "—"}
                    </td>
                  )}
                  <td style={{ padding: "0.65rem 0.5rem", textAlign: "right", fontWeight: 600, fontSize: "0.9rem" }}>
                    {qtyFormatted}
                  </td>
                  <td style={{ padding: "0.65rem 0.5rem", textAlign: "right", fontSize: "0.85rem" }}>
                    {formatPaise(item.unit_price_paise)}
                  </td>
                  <td style={{ padding: "0.65rem 0.5rem", textAlign: "right", fontSize: "0.85rem", color: item.discount_paise > 0 ? "var(--accent)" : "#9ca3af" }}>
                    {item.discount_paise > 0 ? formatPaise(item.discount_paise) : "—"}
                  </td>
                  <td style={{ padding: "0.65rem 0.5rem", textAlign: "right", fontSize: "0.85rem" }}>
                    {(item.gst_rate_bps / 100).toFixed(0)}%
                  </td>
                  <td style={{ padding: "0.65rem 0.5rem", textAlign: "right", fontWeight: 700, fontSize: "0.9rem", color: "#111827" }}>
                    {formatPaise(item.line_total_paise)}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      {/* Tax Breakdown & Totals */}
      <div
        className="avoid-break"
        style={{
          display: "grid",
          gridTemplateColumns: showTaxBreakdown && slabList.length > 0 ? "1.2fr 1fr" : "1fr",
          gap: "1.5rem",
          alignItems: "start",
          marginBottom: "1.5rem",
          position: "relative",
          zIndex: 2,
        }}
      >
        {/* Tax Slab Summary Table */}
        {showTaxBreakdown && slabList.length > 0 ? (
          <div>
            <div style={{ fontSize: "0.75rem", textTransform: "uppercase", fontWeight: 700, color: "#6b7280", letterSpacing: "0.05em", marginBottom: "0.5rem" }}>
              Tax Breakdown (GST)
            </div>
            <table style={{ width: "100%", fontSize: "0.8rem", borderCollapse: "collapse" }}>
              <thead>
                <tr style={{ borderBottom: "1px solid #e5e7eb", background: "#f9fafb" }}>
                  <th style={{ padding: "0.35rem 0.4rem", textAlign: "left" }}>GST Rate</th>
                  <th style={{ padding: "0.35rem 0.4rem", textAlign: "right" }}>Taxable Amount</th>
                  <th style={{ padding: "0.35rem 0.4rem", textAlign: "right" }}>Tax Amount (GST)</th>
                </tr>
              </thead>
              <tbody>
                {slabList.map((slab) => (
                  <tr key={slab.rateBps} style={{ borderBottom: "1px solid #f3f4f6" }}>
                    <td style={{ padding: "0.35rem 0.4rem", fontWeight: 600 }}>{(slab.rateBps / 100).toFixed(0)}%</td>
                    <td style={{ padding: "0.35rem 0.4rem", textAlign: "right" }}>{formatPaise(slab.taxablePaise)}</td>
                    <td style={{ padding: "0.35rem 0.4rem", textAlign: "right", fontWeight: 600 }}>{formatPaise(slab.taxPaise)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <div />
        )}

        {/* Financial Summary */}
        <div style={{ border: "1px solid #e5e7eb", borderRadius: "6px", padding: "1rem", backgroundColor: "#f9fafb" }}>
          <div style={{ display: "flex", justifyContent: "space-between", padding: "0.25rem 0", fontSize: "0.9rem" }}>
            <span style={{ color: "#6b7280" }}>Subtotal:</span>
            <span>{formatPaise(sale.subtotal_paise)}</span>
          </div>
          {sale.discount_paise > 0 && (
            <div style={{ display: "flex", justifyContent: "space-between", padding: "0.25rem 0", fontSize: "0.9rem", color: "var(--accent)" }}>
              <span>Bill Discount:</span>
              <span>- {formatPaise(sale.discount_paise)}</span>
            </div>
          )}
          <div style={{ display: "flex", justifyContent: "space-between", padding: "0.25rem 0", fontSize: "0.9rem" }}>
            <span style={{ color: "#6b7280" }}>Total Tax (GST):</span>
            <span>{formatPaise(sale.tax_total_paise)}</span>
          </div>
          <div
            style={{
              display: "flex",
              justifyContent: "space-between",
              padding: "0.6rem 0 0.2rem",
              marginTop: "0.5rem",
              borderTop: "2px solid #e5e7eb",
              fontSize: "1.25rem",
              fontWeight: 800,
              color: "#111827",
            }}
          >
            <span>Grand Total:</span>
            <span>{formatPaise(sale.grand_total_paise)}</span>
          </div>
        </div>
      </div>

      {/* Amount in Words */}
      <div
        className="avoid-break"
        style={{
          backgroundColor: "#f9fafb",
          border: "1px solid #e5e7eb",
          borderRadius: "6px",
          padding: "0.65rem 1rem",
          marginBottom: "1.5rem",
          position: "relative",
          zIndex: 2,
        }}
      >
        <span style={{ fontSize: "0.75rem", textTransform: "uppercase", fontWeight: 700, color: "#6b7280", letterSpacing: "0.05em", marginRight: "0.5rem" }}>
          Amount in Words:
        </span>
        <strong style={{ fontSize: "0.95rem", color: "#111827" }}>
          {amountToWords(sale.grand_total_paise)}
        </strong>
      </div>

      {/* Terms, Footer & Signature Block */}
      <div
        className="avoid-break"
        style={{
          borderTop: "1px dashed #d1d5db",
          paddingTop: "1.25rem",
          marginTop: "1.5rem",
          display: "grid",
          gridTemplateColumns: "1.3fr 1fr",
          gap: "2rem",
          position: "relative",
          zIndex: 2,
        }}
      >
        <div>
          {settings?.terms_and_conditions ? (
            <div>
              <div style={{ fontSize: "0.75rem", textTransform: "uppercase", fontWeight: 700, color: "#6b7280", letterSpacing: "0.05em", marginBottom: "0.25rem" }}>
                Terms & Conditions
              </div>
              <p style={{ margin: 0, fontSize: "0.8rem", color: "#4b5563", whiteSpace: "pre-line", lineHeight: 1.4 }}>
                {settings.terms_and_conditions}
              </p>
            </div>
          ) : (
            <div>
              <div style={{ fontSize: "0.75rem", textTransform: "uppercase", fontWeight: 700, color: "#6b7280", letterSpacing: "0.05em", marginBottom: "0.25rem" }}>
                Terms & Conditions
              </div>
              <p style={{ margin: 0, fontSize: "0.8rem", color: "#6b7280", lineHeight: 1.4 }}>
                1. Goods once sold will not be accepted without original bill.<br />
                2. Subject to local jurisdiction.
              </p>
            </div>
          )}

          {settings?.invoice_footer && (
            <div style={{ marginTop: "0.75rem", fontSize: "0.85rem", color: "#374151", fontStyle: "italic" }}>
              {settings.invoice_footer}
            </div>
          )}
        </div>

        <div style={{ textAlign: "right", display: "flex", flexDirection: "column", justifyContent: "space-between", minHeight: "90px" }}>
          <div style={{ fontSize: "0.85rem", fontWeight: 600, color: "#374151" }}>
            For {businessName}
          </div>
          <div style={{ borderTop: "1px solid #9ca3af", paddingTop: "0.35rem", width: "180px", marginLeft: "auto" }}>
            <span style={{ fontSize: "0.8rem", color: "#6b7280" }}>Authorized Signatory</span>
          </div>
        </div>
      </div>

      {/* Computer generated disclaimer */}
      <div
        className="avoid-break"
        style={{
          marginTop: "1.25rem",
          textAlign: "center",
          fontSize: "0.75rem",
          color: "#9ca3af",
          position: "relative",
          zIndex: 2,
        }}
      >
        This is a computer generated document and does not require a physical signature unless required by local law.
      </div>
    </div>
  );
}
