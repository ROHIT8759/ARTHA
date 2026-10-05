"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";
import { ApiError } from "@/lib/api";
import { useAuth } from "@/lib/auth-context";

/** First-run screen: creates the business and its owner account.
 *  The backend rejects this with 409 once a business already exists, so
 *  this page is self-limiting even if someone bookmarks the URL. */
export default function SetupPage() {
  const router = useRouter();
  const { setupBusiness } = useAuth();
  const [businessName, setBusinessName] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);

    if (password !== confirm) {
      setError("Passwords do not match.");
      return;
    }

    setSubmitting(true);
    try {
      await setupBusiness(businessName.trim(), username.trim(), password);
      router.replace("/dashboard");
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Could not reach the server. Is it running?");
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <main style={{ display: "flex", minHeight: "100vh", alignItems: "center", justifyContent: "center", padding: "1rem" }}>
      <form onSubmit={onSubmit} className="card" style={{ width: "100%", maxWidth: 420 }}>
        <h1 style={{ marginTop: 0 }}>Set up your shop</h1>
        <p className="muted">This runs once, on first start. You&apos;ll become the owner account.</p>

        <div className="field">
          <label htmlFor="businessName">Business name</label>
          <input id="businessName" required value={businessName} onChange={(e) => setBusinessName(e.target.value)} />
        </div>
        <div className="field">
          <label htmlFor="username">Your username</label>
          <input id="username" required value={username} onChange={(e) => setUsername(e.target.value)} />
        </div>
        <div className="field">
          <label htmlFor="password">Password (min 8 characters)</label>
          <input id="password" type="password" required minLength={8} value={password} onChange={(e) => setPassword(e.target.value)} />
        </div>
        <div className="field">
          <label htmlFor="confirm">Confirm password</label>
          <input id="confirm" type="password" required minLength={8} value={confirm} onChange={(e) => setConfirm(e.target.value)} />
        </div>

        {error && <p className="error-text">{error}</p>}

        <button className="btn" type="submit" disabled={submitting} style={{ width: "100%" }}>
          {submitting ? "Creating…" : "Create business & owner account"}
        </button>
      </form>
    </main>
  );
}
