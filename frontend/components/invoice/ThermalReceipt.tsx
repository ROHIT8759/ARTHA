"use client";

import React from "react";
import { SaleView, BusinessView, formatPaise, amountToWords, computeTaxSlabs, formatGstRate } from "@/lib/api";

interface ThermalReceiptProps {
  sale: SaleView;
  business: BusinessView | null;
}

export default function ThermalReceipt({ sale, business }: ThermalReceiptProps) {
  const isVoid = sale.status === "void";
  const isDraft = sale.status === "draft";
  const settings = business?.settings;

  const showHsn = settings?.show_hsn !== false;
  const showLogo = settings?.show_logo !== false && !!settings?.logo_data_url;
  const showTaxBreakdown = settings?.show_tax_breakdown !== false;

  const businessName = settings?.registered_name?.trim() || business?.name?.trim() || "ARTHA STORE";
  const addressParts = [
    settings?.address,
    settings?.city,
    settings?.state,
    settings?.pincode,
  ].filter(Boolean);
  const formattedAddress = addressParts.join(", ");

  const invoiceDate = sale.finalized_at
    ? new Date(sale.finalized_at).toLocaleString("en-IN", {
        dateStyle: "short",
        timeStyle: "short",
      })
    : new Date(sale.created_at).toLocaleString("en-IN", {
        dateStyle: "short",
        timeStyle: "short",
      });

  // Authoritative tax breakdown from persisted item line records using shared tested helper
  const taxSlabs = computeTaxSlabs(sale.items);
  const slabList = Object.values(taxSlabs).sort((a, b) => a.rateBps - b.rateBps);

  return (
    <div
      className="thermal-receipt-container avoid-break"
      style={{
        width: "320px",
        margin: "0 auto",
        backgroundColor: "#ffffff",
        color: "#000000",
        fontFamily: "'Courier New', Courier, monospace",
        fontSize: "12px",
        lineHeight: 1.35,
        padding: "16px 12px",
        border: "1px dashed #9ca3af",
        boxShadow: "0 2px 8px rgba(0,0,0,0.06)",
        borderRadius: "4px",
      }}
    >
      {/* Centered Store Header */}
      <div style={{ textAlign: "center", marginBottom: "8px" }}>
        {showLogo && settings?.logo_data_url && (
          <div style={{ marginBottom: "6px" }}>
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img
              src={settings.logo_data_url}
              alt={businessName}
              style={{
                maxHeight: "42px",
                maxWidth: "140px",
                objectFit: "contain",
                margin: "0 auto",
                display: "block",
              }}
            />
          </div>
        )}
        <div style={{ fontSize: "15px", fontWeight: "bold", textTransform: "uppercase", letterSpacing: "0.5px" }}>
          {businessName}
        </div>
        {formattedAddress && (
          <div style={{ fontSize: "11px", marginTop: "2px", color: "#333" }}>
            {formattedAddress}
          </div>
        )}
        {settings?.phone && (
          <div style={{ fontSize: "11px", marginTop: "1px" }}>
            Ph: {settings.phone}
          </div>
        )}
        {settings?.gstin && (
          <div style={{ fontSize: "11px", marginTop: "2px", fontWeight: "bold" }}>
            GSTIN: {settings.gstin}
          </div>
        )}
      </div>

      <div style={{ borderTop: "1px dashed #000", margin: "6px 0" }} />

      {/* Document Title & Status */}
      <div style={{ textAlign: "center", fontWeight: "bold" }}>
        {isDraft && <span style={{ color: "#b45309" }}>*** DRAFT - NOT A BILL ***</span>}
        {isVoid && <span style={{ color: "#dc2626" }}>*** VOID - CANCELLED (NO REFUND) ***</span>}
        {!isDraft && !isVoid && <span>*** TAX INVOICE ***</span>}
      </div>

      <div style={{ borderTop: "1px dashed #000", margin: "6px 0" }} />

      {/* Invoice Meta */}
      <div style={{ fontSize: "11px" }}>
        <div style={{ display: "flex", justifyContent: "space-between" }}>
          <span>Inv: <strong>{sale.invoice_no || "DRAFT"}</strong></span>
          <span>{invoiceDate}</span>
        </div>
        <div style={{ display: "flex", justifyContent: "space-between", marginTop: "2px" }}>
          <span>Customer: {sale.customer_name || "Walk-in"}</span>
          <span>Mode: {sale.payment_method.toUpperCase()}{isVoid && sale.payment_status === "paid" ? " (UNREFUNDED)" : ""}</span>
        </div>
        {sale.customer_phone && (
          <div style={{ marginTop: "1px" }}>
            Ph: {sale.customer_phone}
          </div>
        )}
        {sale.customer_gstin && (
          <div style={{ marginTop: "1px" }}>
            GSTIN: {sale.customer_gstin}
          </div>
        )}
        {sale.finalized_by && (
          <div style={{ marginTop: "1px", color: "#444" }}>
            Staff: {sale.finalized_by}
          </div>
        )}
      </div>

      <div style={{ borderTop: "1px dashed #000", margin: "6px 0" }} />

      {/* Items Header */}
      <div style={{ display: "flex", justifyContent: "space-between", fontWeight: "bold", fontSize: "11px" }}>
        <span style={{ flex: 1 }}>ITEM / QTY x RATE</span>
        <span style={{ textAlign: "right", width: "70px" }}>AMOUNT</span>
      </div>

      <div style={{ borderTop: "1px solid #000", margin: "4px 0" }} />

      {/* Line Items */}
      <div style={{ margin: "4px 0" }}>
        {sale.items.map((item, idx) => {
          const qty = (item.quantity_milli / 1000).toFixed(
            item.quantity_milli % 1000 === 0 ? 0 : 2
          );

          return (
            <div key={item.sale_item_id || idx} style={{ marginBottom: "6px" }}>
              <div style={{ fontWeight: "bold", wordBreak: "break-word" }}>
                {idx + 1}. {item.product_name_snapshot}
              </div>
              <div style={{ display: "flex", justifyContent: "space-between", fontSize: "11px", color: "#222", paddingLeft: "12px" }}>
                <span>
                  {qty} x {formatPaise(item.unit_price_paise)}
                  {showHsn && item.hsn_code_snapshot ? ` (H:${item.hsn_code_snapshot})` : ""}
                  {item.discount_paise > 0 ? ` [-${formatPaise(item.discount_paise)}]` : ""}
                </span>
                <span style={{ fontWeight: "bold" }}>
                  {formatPaise(item.line_total_paise)}
                </span>
              </div>
            </div>
          );
        })}
      </div>

      <div style={{ borderTop: "1px dashed #000", margin: "6px 0" }} />

      {/* Totals */}
      <div style={{ fontSize: "12px" }}>
        <div style={{ display: "flex", justifyContent: "space-between", marginBottom: "2px" }}>
          <span>Subtotal:</span>
          <span>{formatPaise(sale.subtotal_paise)}</span>
        </div>
        {sale.discount_paise > 0 && (
          <div style={{ display: "flex", justifyContent: "space-between", marginBottom: "2px" }}>
            <span>Discount:</span>
            <span>- {formatPaise(sale.discount_paise)}</span>
          </div>
        )}
        <div style={{ display: "flex", justifyContent: "space-between", marginBottom: "2px" }}>
          <span>Tax (GST):</span>
          <span>{formatPaise(sale.tax_total_paise)}</span>
        </div>
        <div style={{ borderTop: "1px solid #000", margin: "4px 0" }} />
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            fontSize: "15px",
            fontWeight: "bold",
            margin: "4px 0",
          }}
        >
          <span>TOTAL:</span>
          <span>{formatPaise(sale.grand_total_paise)}</span>
        </div>
        <div style={{ borderTop: "1px solid #000", margin: "4px 0" }} />
      </div>

      {/* Amount in words */}
      <div style={{ fontSize: "10px", marginTop: "4px", color: "#333", wordBreak: "break-word" }}>
        In Words: <strong>{amountToWords(sale.grand_total_paise)}</strong>
      </div>

      {/* Tax breakdown summary */}
      {showTaxBreakdown && slabList.length > 0 && (
        <div style={{ fontSize: "10px", marginTop: "6px", borderTop: "1px dotted #888", paddingTop: "4px" }}>
          <div style={{ fontWeight: "bold" }}>GST Summary:</div>
          {slabList.map((slab) => (
            <div key={slab.rateBps} style={{ display: "flex", justifyContent: "space-between" }}>
              <span>
                {formatGstRate(slab.rateBps)} on{" "}
                {formatPaise(slab.taxablePaise)}:
              </span>
              <span>{formatPaise(slab.taxPaise)}</span>
            </div>
          ))}
        </div>
      )}

      <div style={{ borderTop: "1px dashed #000", margin: "8px 0" }} />

      {/* Terms & Footer Note */}
      <div style={{ textAlign: "center", fontSize: "10px", color: "#333" }}>
        {settings?.invoice_footer && (
          <div style={{ marginBottom: "4px" }}>
            {settings.invoice_footer}
          </div>
        )}
        {settings?.terms_and_conditions ? (
          <div style={{ fontSize: "9px", color: "#555", whiteSpace: "pre-line", marginBottom: "4px" }}>
            {settings.terms_and_conditions}
          </div>
        ) : (
          <div style={{ fontSize: "9px", color: "#555", marginBottom: "4px" }}>
            Goods once sold will not be exchanged without bill.
          </div>
        )}
        <div style={{ fontWeight: "bold", marginTop: "4px" }}>
          *** THANK YOU! VISIT AGAIN ***
        </div>
      </div>
    </div>
  );
}
