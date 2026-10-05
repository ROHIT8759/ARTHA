"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { useEffect } from "react";
import { useAuth } from "@/lib/auth-context";

export default function DashboardPage() {
  const router = useRouter();
  const { user, loading, logout } = useAuth();

  useEffect(() => {
    if (!loading && !user) router.replace("/login");
  }, [loading, user, router]);

  if (loading || !user) return null;

  return (
    <main style={{ padding: "2rem", maxWidth: 720, margin: "0 auto" }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline" }}>
        <h1>ARTHA</h1>
        <button
          className="btn btn-secondary"
          onClick={async () => {
            await logout();
            router.replace("/login");
          }}
        >
          Sign out
        </button>
      </div>
      <p className="muted">
        Signed in as <strong>{user.username}</strong> ({user.role})
      </p>

      <nav style={{ display: "grid", gap: "0.75rem", marginTop: "2rem" }}>
        <Link className="card" href="/products">
          Products — catalog &amp; QR codes
        </Link>
        {/* Sales (QR-first billing) and Purchases (manual + OCR) are V0.1/V0.2
            workflows; this dashboard links only to what's implemented in V0. */}
      </nav>
    </main>
  );
}
