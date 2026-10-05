"use client";

import { useState } from "react";
import { ProductView } from "@/lib/api";

export interface ProductFormData {
  name: string;
  batch?: string;
  hsn_code?: string;
  gst_rate_bps: number;
  qr_code: string;
  price_paise: number;
  cost_paise: number;
  unit: string;
}

interface ProductFormProps {
  initialData?: ProductView;
  onSubmit: (data: ProductFormData) => Promise<void>;
  onCancel: () => void;
  submitting: boolean;
  error: string | null;
}

export default function ProductForm({ initialData, onSubmit, onCancel, submitting, error }: ProductFormProps) {
  const [name, setName] = useState(initialData?.name || "");
  const [batch, setBatch] = useState(initialData?.batch || "");
  const [hsnCode, setHsnCode] = useState(initialData?.hsn_code || "");
  const [gstRate, setGstRate] = useState(initialData ? (initialData.gst_rate_bps / 100).toString() : "0");
  const [qrCode, setQrCode] = useState(initialData?.qr_code || "");
  const [price, setPrice] = useState(initialData ? (initialData.price_paise / 100).toString() : "");
  const [cost, setCost] = useState(initialData ? (initialData.cost_paise / 100).toString() : "");
  const [unit, setUnit] = useState(initialData?.unit || "pcs");

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    onSubmit({
      name,
      batch: batch || undefined,
      hsn_code: hsnCode || undefined,
      gst_rate_bps: Math.round(parseFloat(gstRate) * 100),
      qr_code: qrCode,
      price_paise: Math.round(parseFloat(price) * 100),
      cost_paise: Math.round(parseFloat(cost) * 100),
      unit
    });
  };

  return (
    <form onSubmit={handleSubmit} className="card" style={{ maxWidth: 600 }}>
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "1rem" }}>
        <div className="field" style={{ gridColumn: "1 / -1" }}>
          <label>Product Name *</label>
          <input required value={name} onChange={e => setName(e.target.value)} />
        </div>
        
        <div className="field">
          <label>QR Code / Barcode *</label>
          <input required value={qrCode} onChange={e => setQrCode(e.target.value)} placeholder="Scan or type..." />
        </div>
        
        <div className="field">
          <label>Batch (Optional)</label>
          <input value={batch} onChange={e => setBatch(e.target.value)} />
        </div>
        
        <div className="field">
          <label>Selling Price (₹) *</label>
          <input type="number" step="0.01" min="0" required value={price} onChange={e => setPrice(e.target.value)} />
        </div>
        
        <div className="field">
          <label>Purchase Cost (₹) *</label>
          <input type="number" step="0.01" min="0" required value={cost} onChange={e => setCost(e.target.value)} />
        </div>
        
        <div className="field">
          <label>GST Rate (%) *</label>
          <input type="number" step="0.01" min="0" required value={gstRate} onChange={e => setGstRate(e.target.value)} />
        </div>
        
        <div className="field">
          <label>HSN Code (Optional)</label>
          <input value={hsnCode} onChange={e => setHsnCode(e.target.value)} />
        </div>
        
        <div className="field">
          <label>Unit *</label>
          <select value={unit} onChange={e => setUnit(e.target.value)}>
            <option value="pcs">Pieces (pcs)</option>
            <option value="kg">Kilograms (kg)</option>
            <option value="ltr">Liters (ltr)</option>
            <option value="mtr">Meters (mtr)</option>
            <option value="box">Box (box)</option>
          </select>
        </div>
      </div>

      {error && <p className="error-text" style={{ marginTop: "1rem" }}>{error}</p>}

      <div style={{ display: "flex", gap: "1rem", marginTop: "2rem" }}>
        <button type="button" className="btn btn-secondary" onClick={onCancel} disabled={submitting}>
          Cancel
        </button>
        <button type="submit" className="btn" disabled={submitting}>
          {submitting ? "Saving..." : "Save Product"}
        </button>
      </div>
    </form>
  );
}
