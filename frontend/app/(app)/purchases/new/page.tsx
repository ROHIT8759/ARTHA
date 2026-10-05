"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";
import Link from "next/link";
import { api } from "@/lib/api";

export default function NewPurchasePage() {
  const router = useRouter();
  const [loading, setLoading] = useState<"manual" | "ocr" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const createDraft = async (source: "manual" | "ocr") => {
    setLoading(source);
    setError(null);
    try {
      const draft = await api.createPurchase(source);
      router.push(`/purchases/${draft.purchase_id}`);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to create draft");
      setLoading(null);
    }
  };

  return (
    <div style={{ maxWidth: 600, margin: "0 auto" }}>
      <div style={{ marginBottom: "2rem" }}>
        <Link href="/purchases" className="btn btn-secondary" style={{ marginBottom: "1rem", display: "inline-block" }}>
          &larr; Back to Purchases
        </Link>
        <h1 style={{ margin: 0 }}>New Purchase</h1>
        <p className="muted">How would you like to enter this purchase?</p>
      </div>
      
      {error && (
        <div className="card" style={{ marginBottom: "1rem", borderColor: "var(--danger)", backgroundColor: "rgba(179, 38, 30, 0.05)" }}>
          <p style={{ color: "var(--danger)", margin: 0 }}>{error}</p>
        </div>
      )}

      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "1rem" }}>
        <div className="card" style={{ textAlign: "center", cursor: loading ? "wait" : "pointer" }} onClick={() => !loading && createDraft("manual")}>
          <h2>Manual Entry</h2>
          <p className="muted">Type in the invoice details and line items yourself.</p>
          <button className="btn" style={{ marginTop: "1rem" }} disabled={loading !== null}>
            {loading === "manual" ? "Creating..." : "Start Manual Entry"}
          </button>
        </div>
        
        <div className="card" style={{ textAlign: "center", cursor: loading ? "wait" : "pointer", border: "2px solid var(--accent)" }} onClick={() => !loading && createDraft("ocr")}>
          <h2>Scan Bill Photo</h2>
          <p className="muted">Upload or snap a photo of the bill. The system will auto-extract items.</p>
          <button className="btn" style={{ marginTop: "1rem" }} disabled={loading !== null}>
            {loading === "ocr" ? "Creating..." : "Start Smart Scan"}
          </button>
        </div>
      </div>
    </div>
  );
}
