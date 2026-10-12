"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import Link from "next/link";
import { api } from "@/lib/api";
import LoadingState from "@/components/ui/LoadingState";

export default function NewSalePage() {
  const router = useRouter();
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let mounted = true;
    const initDraft = async () => {
      try {
        const draft = await api.createSale({ payment_method: "cash" });
        if (mounted) {
          router.replace(`/sales/${draft.sale_id}`);
        }
      } catch (err: unknown) {
        if (mounted) {
          setError(err instanceof Error ? err.message : "Failed to initialize new sale");
        }
      }
    };
    initDraft();
    return () => {
      mounted = false;
    };
  }, [router]);

  if (error) {
    return (
      <div style={{ maxWidth: 500, margin: "2rem auto" }}>
        <div className="card" style={{ borderColor: "var(--danger)", backgroundColor: "rgba(179, 38, 30, 0.05)" }}>
          <h2 style={{ color: "var(--danger)", marginTop: 0 }}>Unable to create sale</h2>
          <p>{error}</p>
          <Link href="/sales" className="btn btn-secondary" style={{ marginTop: "1rem", display: "inline-block" }}>
            &larr; Back to Sales
          </Link>
        </div>
      </div>
    );
  }

  return (
    <div style={{ maxWidth: 400, margin: "4rem auto", textAlign: "center" }}>
      <LoadingState />
      <p className="muted" style={{ marginTop: "1rem" }}>Starting POS checkout session...</p>
    </div>
  );
}
