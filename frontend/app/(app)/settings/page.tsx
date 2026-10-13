"use client";

import { useEffect, useState, useRef } from "react";
import { useAuth } from "@/lib/auth-context";
import { api, setToken, BusinessView, BackupView, BackupVerifyResult, formatBytes, formatRestoreErrorMessage } from "@/lib/api";
import LoadingState from "@/components/ui/LoadingState";
import ErrorState from "@/components/ui/ErrorState";

export default function SettingsPage() {
  const { user } = useAuth();

  const [business, setBusiness] = useState<BusinessView | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [feedback, setFeedback] = useState<{ type: "success" | "error"; text: string } | null>(null);

  // Form fields
  const [name, setName] = useState("");
  const [registeredName, setRegisteredName] = useState("");
  const [address, setAddress] = useState("");
  const [city, setCity] = useState("");
  const [state, setState] = useState("");
  const [pincode, setPincode] = useState("");
  const [country, setCountry] = useState("India");
  const [phone, setPhone] = useState("");
  const [email, setEmail] = useState("");
  const [gstin, setGstin] = useState("");
  const [invoiceFooter, setInvoiceFooter] = useState("");
  const [terms, setTerms] = useState("");
  const [defaultLayout, setDefaultLayout] = useState<"a4" | "thermal">("a4");
  const [showHsn, setShowHsn] = useState(true);
  const [showTaxBreakdown, setShowTaxBreakdown] = useState(true);
  const [showLogo, setShowLogo] = useState(true);

  // Logo state
  const [logoUploading, setLogoUploading] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Backup & restore state
  const [backups, setBackups] = useState<BackupView[]>([]);
  const [backupsLoading, setBackupsLoading] = useState(false);
  const [backupsError, setBackupsError] = useState<string | null>(null);
  const [creatingBackup, setCreatingBackup] = useState(false);
  const [backupActionId, setBackupActionId] = useState<string | null>(null);
  const [backupActionKind, setBackupActionKind] = useState<"verify" | "restore" | null>(null);
  const [verifyResults, setVerifyResults] = useState<Record<string, BackupVerifyResult>>({});
  const [backupFeedback, setBackupFeedback] = useState<{ type: "success" | "error"; text: string } | null>(null);

  useEffect(() => {
    let mounted = true;
    const loadBusiness = async () => {
      try {
        setLoading(true);
        const data = await api.getBusiness();
        if (mounted) {
          setBusiness(data);
          setName(data.name || "");
          setRegisteredName(data.settings.registered_name || "");
          setAddress(data.settings.address || "");
          setCity(data.settings.city || "");
          setState(data.settings.state || "");
          setPincode(data.settings.pincode || "");
          setCountry(data.settings.country || "India");
          setPhone(data.settings.phone || "");
          setEmail(data.settings.email || "");
          setGstin(data.settings.gstin || "");
          setInvoiceFooter(data.settings.invoice_footer || "");
          setTerms(data.settings.terms_and_conditions || "");
          setDefaultLayout((data.settings.default_layout as "a4" | "thermal") || "a4");
          setShowHsn(data.settings.show_hsn !== false);
          setShowTaxBreakdown(data.settings.show_tax_breakdown !== false);
          setShowLogo(data.settings.show_logo !== false);
          setError(null);
        }
      } catch (err: unknown) {
        if (mounted) {
          setError(err instanceof Error ? err.message : "Failed to load business settings");
        }
      } finally {
        if (mounted) {
          setLoading(false);
        }
      }
    };

    if (user?.role === "owner") {
      loadBusiness();
    }

    return () => {
      mounted = false;
    };
  }, [user?.role]);

  const loadBackups = async () => {
    try {
      setBackupsLoading(true);
      const data = await api.listBackups();
      setBackups(data);
      setBackupsError(null);
    } catch (err: unknown) {
      setBackupsError(err instanceof Error ? err.message : "Failed to load backups");
    } finally {
      setBackupsLoading(false);
    }
  };

  useEffect(() => {
    let mounted = true;
    const load = async () => {
      try {
        setBackupsLoading(true);
        const data = await api.listBackups();
        if (mounted) {
          setBackups(data);
          setBackupsError(null);
        }
      } catch (err: unknown) {
        if (mounted) {
          setBackupsError(err instanceof Error ? err.message : "Failed to load backups");
        }
      } finally {
        if (mounted) setBackupsLoading(false);
      }
    };

    if (user?.role === "owner") {
      load();
    }

    return () => {
      mounted = false;
    };
  }, [user?.role]);

  const handleCreateBackup = async () => {
    setCreatingBackup(true);
    setBackupFeedback(null);
    try {
      const created = await api.createBackup();
      setBackups((prev) => [created, ...prev]);
      setBackupFeedback({ type: "success", text: `Backup created (${formatBytes(created.size_bytes)}).` });
    } catch (err: unknown) {
      setBackupFeedback({
        type: "error",
        text: err instanceof Error ? err.message : "Failed to create backup",
      });
    } finally {
      setCreatingBackup(false);
    }
  };

  const handleVerifyBackup = async (backupId: string) => {
    setBackupActionId(backupId);
    setBackupActionKind("verify");
    setBackupFeedback(null);
    try {
      const result = await api.verifyBackup(backupId);
      setVerifyResults((prev) => ({ ...prev, [backupId]: result }));
    } catch (err: unknown) {
      setBackupFeedback({
        type: "error",
        text: err instanceof Error ? err.message : "Failed to verify backup",
      });
    } finally {
      setBackupActionId(null);
      setBackupActionKind(null);
    }
  };

  const handleRestoreBackup = async (backup: BackupView) => {
    const confirmed = confirm(
      `Restore the database to the state from ${new Date(backup.created_at).toLocaleString()}?\n\n` +
        "This replaces ALL current data (products, sales, purchases, settings) with that backup's contents. " +
        "A safety backup of the current state is taken automatically first, so this can be undone by restoring " +
        "that safety backup afterward.\n\nThis cannot be skipped or undone immediately. Continue?"
    );
    if (!confirmed) return;

    setBackupActionId(backup.backup_id);
    setBackupActionKind("restore");
    setBackupFeedback(null);
    try {
      const result = await api.restoreBackup(backup.backup_id, true);
      if (result.current_session_still_valid) {
        setBackupFeedback({
          type: "success",
          text: `Database restored. A safety backup of the prior state was saved (id: ${result.safety_backup_id}). Reloading...`,
        });
        // Virtually every piece of app state (products, sales, settings)
        // may have just changed underneath the running app; a full reload
        // is the only way to guarantee nothing stale is shown.
        setTimeout(() => window.location.reload(), 1500);
      } else {
        // The restore replaced the businesses/users/sessions tables with an
        // older snapshot that predates this account (or predates setup
        // entirely) — the current login no longer exists post-restore.
        // Reloading would just bounce to an unexplained login screen, so
        // tell the owner plainly first, then send them to re-authenticate
        // (or redo setup, if the restored snapshot predates it).
        setBackupFeedback({
          type: "success",
          text: `Database restored to an earlier state. A safety backup of the prior state was saved (id: ${result.safety_backup_id}). Your current login no longer exists in the restored data — redirecting to sign in again...`,
        });
        setTimeout(() => {
          setToken(null);
          // A deliberate hard navigation, not client-side routing: a
          // restore can also invalidate in-memory AuthContext state, so a
          // full reload is needed here for the same reason as the
          // reload-on-success branch above.
          // eslint-disable-next-line @next/next/no-location-assign-relative-destination
          window.location.href = "/login";
        }, 2500);
      }
    } catch (err: unknown) {
      setBackupFeedback({
        type: "error",
        text: formatRestoreErrorMessage(err),
      });
      setBackupActionId(null);
      setBackupActionKind(null);
    }
  };

  if (user?.role !== "owner") {
    return (
      <div style={{ maxWidth: 600, margin: "2rem auto" }}>
        <ErrorState
          title="Owner Access Required"
          message="Business details and invoice layout settings can only be managed by the shop owner."
        />
      </div>
    );
  }

  if (loading && !business) return <LoadingState />;
  if (error && !business) return <ErrorState message={error} onRetry={() => window.location.reload()} />;

  const handleSave = async (e: React.FormEvent) => {
    e.preventDefault();
    setSaving(true);
    setFeedback(null);

    try {
      const updated = await api.updateBusiness({
        name,
        registered_name: registeredName,
        address,
        city,
        state,
        pincode,
        country,
        phone,
        email,
        gstin,
        invoice_footer: invoiceFooter,
        terms_and_conditions: terms,
        default_layout: defaultLayout,
        show_hsn: showHsn,
        show_tax_breakdown: showTaxBreakdown,
        show_logo: showLogo,
      });

      setBusiness(updated);
      setFeedback({ type: "success", text: "Settings saved successfully!" });
    } catch (err: unknown) {
      setFeedback({
        type: "error",
        text: err instanceof Error ? err.message : "Failed to update settings",
      });
    } finally {
      setSaving(false);
    }
  };

  const handleLogoUpload = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    // Client-side file type & size check
    if (!file.type.startsWith("image/") || file.type.includes("svg")) {
      alert("Only raster images (PNG, JPEG, WebP) are allowed.");
      return;
    }

    if (file.size > 1024 * 1024) {
      alert("Logo image size must be under 1MB.");
      return;
    }

    setLogoUploading(true);
    const reader = new FileReader();
    reader.onload = async () => {
      try {
        const dataUrl = reader.result as string;
        const updated = await api.uploadLogo(dataUrl);
        setBusiness(updated);
        setFeedback({ type: "success", text: "Logo uploaded successfully!" });
      } catch (err: unknown) {
        setFeedback({
          type: "error",
          text: err instanceof Error ? err.message : "Failed to upload logo",
        });
      } finally {
        setLogoUploading(false);
        if (fileInputRef.current) fileInputRef.current.value = "";
      }
    };
    reader.readAsDataURL(file);
  };

  const handleRemoveLogo = async () => {
    if (!confirm("Are you sure you want to remove the business logo?")) return;
    setLogoUploading(true);
    try {
      const updated = await api.removeLogo();
      setBusiness(updated);
      setFeedback({ type: "success", text: "Logo removed." });
    } catch (err: unknown) {
      setFeedback({
        type: "error",
        text: err instanceof Error ? err.message : "Failed to remove logo",
      });
    } finally {
      setLogoUploading(false);
    }
  };

  return (
    <div style={{ maxWidth: 840, margin: "0 auto", paddingBottom: "3rem" }}>
      <div style={{ marginBottom: "1.5rem" }}>
        <h1 style={{ margin: "0 0 0.25rem" }}>Business & Invoice Settings</h1>
        <p className="muted" style={{ margin: 0 }}>
          Configure shop identity, contact details, tax identification, and invoice print formatting
        </p>
      </div>

      {feedback && (
        <div
          className="card"
          style={{
            marginBottom: "1.5rem",
            padding: "0.85rem 1rem",
            borderColor: feedback.type === "success" ? "var(--accent)" : "var(--danger)",
            backgroundColor: feedback.type === "success" ? "rgba(15,107,76,0.08)" : "rgba(179,38,30,0.08)",
          }}
        >
          <strong style={{ color: feedback.type === "success" ? "var(--accent)" : "var(--danger)" }}>
            {feedback.text}
          </strong>
        </div>
      )}

      <form onSubmit={handleSave}>
        {/* Business Identity */}
        <div className="card" style={{ marginBottom: "1.5rem" }}>
          <h2 style={{ fontSize: "1.2rem", marginTop: 0, borderBottom: "1px solid var(--border)", paddingBottom: "0.5rem" }}>
            🏢 Shop & Business Identity
          </h2>

          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "1rem" }}>
            <div className="field">
              <label>Shop / Business Name *</label>
              <input
                type="text"
                required
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g. Artha Supermart"
              />
            </div>
            <div className="field">
              <label>Registered Legal Name (Optional)</label>
              <input
                type="text"
                value={registeredName}
                onChange={(e) => setRegisteredName(e.target.value)}
                placeholder="e.g. Artha Retail Enterprises Pvt Ltd"
              />
            </div>
          </div>

          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "1rem" }}>
            <div className="field">
              <label>GSTIN (Optional)</label>
              <input
                type="text"
                value={gstin}
                onChange={(e) => setGstin(e.target.value.toUpperCase())}
                placeholder="e.g. 29AAAAA0000A1Z5"
              />
            </div>
            <div className="field">
              <label>Phone Number</label>
              <input
                type="tel"
                value={phone}
                onChange={(e) => setPhone(e.target.value)}
                placeholder="e.g. +91 9876543210"
              />
            </div>
          </div>

          <div className="field">
            <label>Email Address</label>
            <input
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder="e.g. support@myshop.in"
            />
          </div>
        </div>

        {/* Address */}
        <div className="card" style={{ marginBottom: "1.5rem" }}>
          <h2 style={{ fontSize: "1.2rem", marginTop: 0, borderBottom: "1px solid var(--border)", paddingBottom: "0.5rem" }}>
            📍 Address & Location
          </h2>

          <div className="field">
            <label>Street Address</label>
            <input
              type="text"
              value={address}
              onChange={(e) => setAddress(e.target.value)}
              placeholder="Shop #12, Market Square"
            />
          </div>

          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: "1rem" }}>
            <div className="field">
              <label>City</label>
              <input
                type="text"
                value={city}
                onChange={(e) => setCity(e.target.value)}
                placeholder="Bengaluru"
              />
            </div>
            <div className="field">
              <label>State</label>
              <input
                type="text"
                value={state}
                onChange={(e) => setState(e.target.value)}
                placeholder="Karnataka"
              />
            </div>
            <div className="field">
              <label>PIN / Postal Code</label>
              <input
                type="text"
                value={pincode}
                onChange={(e) => setPincode(e.target.value)}
                placeholder="560001"
              />
            </div>
          </div>
        </div>

        {/* Logo Management */}
        <div className="card" style={{ marginBottom: "1.5rem" }}>
          <h2 style={{ fontSize: "1.2rem", marginTop: 0, borderBottom: "1px solid var(--border)", paddingBottom: "0.5rem" }}>
            🖼️ Business Logo
          </h2>

          <div style={{ display: "flex", alignItems: "center", gap: "1.5rem" }}>
            {business?.settings.logo_data_url ? (
              <div style={{ display: "flex", alignItems: "center", gap: "1rem" }}>
                {/* eslint-disable-next-line @next/next/no-img-element */}
                <img
                  src={business.settings.logo_data_url}
                  alt="Business Logo"
                  style={{
                    maxHeight: 70,
                    maxWidth: 150,
                    border: "1px solid var(--border)",
                    borderRadius: 4,
                    padding: 4,
                    backgroundColor: "#fff",
                  }}
                />
                <button
                  type="button"
                  onClick={handleRemoveLogo}
                  disabled={logoUploading}
                  className="btn btn-secondary"
                  style={{ color: "var(--danger)", borderColor: "var(--danger)" }}
                >
                  {logoUploading ? "Removing..." : "Remove Logo"}
                </button>
              </div>
            ) : (
              <div>
                <p className="muted" style={{ margin: "0 0 0.5rem", fontSize: "0.9rem" }}>
                  Upload a PNG, JPEG, or WebP logo (under 1MB) to display on A4 and Thermal invoices.
                </p>
                <input
                  ref={fileInputRef}
                  type="file"
                  accept="image/png,image/jpeg,image/webp"
                  onChange={handleLogoUpload}
                  disabled={logoUploading}
                />
              </div>
            )}
          </div>
        </div>

        {/* Invoice & Print Formatting */}
        <div className="card" style={{ marginBottom: "1.5rem" }}>
          <h2 style={{ fontSize: "1.2rem", marginTop: 0, borderBottom: "1px solid var(--border)", paddingBottom: "0.5rem" }}>
            🖨️ Document & Printing Preferences
          </h2>

          <div className="field">
            <label>Default Invoice Print Format</label>
            <select
              value={defaultLayout}
              onChange={(e) => setDefaultLayout(e.target.value as "a4" | "thermal")}
              style={{ maxWidth: 250 }}
            >
              <option value="a4">A4 Full Page Invoice</option>
              <option value="thermal">Compact Thermal Receipt (58mm / 80mm)</option>
            </select>
          </div>

          <div style={{ display: "flex", flexDirection: "column", gap: "0.75rem", marginTop: "1rem" }}>
            <label style={{ display: "flex", alignItems: "center", gap: "0.5rem", cursor: "pointer" }}>
              <input
                type="checkbox"
                checked={showLogo}
                onChange={(e) => setShowLogo(e.target.checked)}
              />
              Display logo on printed documents
            </label>

            <label style={{ display: "flex", alignItems: "center", gap: "0.5rem", cursor: "pointer" }}>
              <input
                type="checkbox"
                checked={showHsn}
                onChange={(e) => setShowHsn(e.target.checked)}
              />
              Show HSN code column on invoices
            </label>

            <label style={{ display: "flex", alignItems: "center", gap: "0.5rem", cursor: "pointer" }}>
              <input
                type="checkbox"
                checked={showTaxBreakdown}
                onChange={(e) => setShowTaxBreakdown(e.target.checked)}
              />
              Show detailed GST tax breakdown
            </label>
          </div>
        </div>

        {/* Notes & Footer */}
        <div className="card" style={{ marginBottom: "2rem" }}>
          <h2 style={{ fontSize: "1.2rem", marginTop: 0, borderBottom: "1px solid var(--border)", paddingBottom: "0.5rem" }}>
            📝 Invoice Footers & Policies
          </h2>

          <div className="field">
            <label>Invoice Footer Greeting</label>
            <input
              type="text"
              value={invoiceFooter}
              onChange={(e) => setInvoiceFooter(e.target.value)}
              placeholder="e.g. Thank you for shopping with us! Please visit again."
            />
          </div>

          <div className="field">
            <label>Terms & Conditions / Return Policy</label>
            <input
              type="text"
              value={terms}
              onChange={(e) => setTerms(e.target.value)}
              placeholder="e.g. Goods once sold can be exchanged within 7 days with original invoice."
            />
          </div>
        </div>

        <button
          type="submit"
          disabled={saving}
          className="btn"
          style={{ padding: "0.75rem 2rem", fontSize: "1rem" }}
        >
          {saving ? "Saving Changes..." : "💾 Save Settings"}
        </button>
      </form>

      {/* Backup & Restore */}
      <div className="card" style={{ marginTop: "2rem" }}>
        <h2 style={{ fontSize: "1.2rem", marginTop: 0, borderBottom: "1px solid var(--border)", paddingBottom: "0.5rem" }}>
          💾 Backup & Restore
        </h2>
        <p className="muted" style={{ fontSize: "0.9rem", marginTop: 0 }}>
          Local backups are taken automatically on a schedule and can also be created on demand. Restoring
          replaces all current data with a backup&apos;s contents; a safety backup of the current state is
          always taken first.
        </p>

        {backupFeedback && (
          <div
            style={{
              marginBottom: "1rem",
              padding: "0.65rem 0.85rem",
              borderRadius: 6,
              border: `1px solid ${backupFeedback.type === "success" ? "var(--accent)" : "var(--danger)"}`,
              backgroundColor: backupFeedback.type === "success" ? "rgba(15,107,76,0.08)" : "rgba(179,38,30,0.08)",
            }}
          >
            <strong style={{ color: backupFeedback.type === "success" ? "var(--accent)" : "var(--danger)" }}>
              {backupFeedback.text}
            </strong>
          </div>
        )}

        <button
          type="button"
          className="btn"
          onClick={handleCreateBackup}
          disabled={creatingBackup}
          style={{ marginBottom: "1.25rem" }}
        >
          {creatingBackup ? "Creating Backup..." : "📦 Create Backup Now"}
        </button>

        {backupsLoading && backups.length === 0 ? (
          <LoadingState message="Loading backups..." />
        ) : backupsError && backups.length === 0 ? (
          <ErrorState message={backupsError} onRetry={loadBackups} />
        ) : backups.length === 0 ? (
          <p className="muted">No backups yet. Create one above, or wait for the next scheduled backup.</p>
        ) : (
          <table>
            <thead>
              <tr>
                <th>Created</th>
                <th>Size</th>
                <th>Schema</th>
                <th>Status</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {backups.map((b) => {
                const verify = verifyResults[b.backup_id];
                const busy = backupActionId === b.backup_id;
                const verifying = busy && backupActionKind === "verify";
                const restoring = busy && backupActionKind === "restore";
                return (
                  <tr key={b.backup_id}>
                    <td>{new Date(b.created_at).toLocaleString()}</td>
                    <td>{formatBytes(b.size_bytes)}</td>
                    <td>v{b.schema_version}</td>
                    <td>
                      {verify === undefined ? (
                        <span className="muted">Not checked</span>
                      ) : verify.valid ? (
                        <span style={{ color: "var(--accent)" }}>✓ Valid</span>
                      ) : (
                        <span style={{ color: "var(--danger)" }}>
                          ✗ Invalid
                          {!verify.checksum_ok ? " (checksum)" : ""}
                          {!verify.integrity_ok ? " (integrity)" : ""}
                        </span>
                      )}
                    </td>
                    <td>
                      <div style={{ display: "flex", gap: "0.5rem" }}>
                        <button
                          type="button"
                          className="btn btn-secondary"
                          disabled={busy}
                          onClick={() => handleVerifyBackup(b.backup_id)}
                        >
                          {verifying ? "..." : "Verify"}
                        </button>
                        <button
                          type="button"
                          className="btn btn-secondary"
                          disabled={busy}
                          style={{ color: "var(--danger)", borderColor: "var(--danger)" }}
                          onClick={() => handleRestoreBackup(b)}
                        >
                          {restoring ? "Restoring..." : "Restore"}
                        </button>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
