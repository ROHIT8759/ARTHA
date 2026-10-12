"use client";

import { useEffect, useState, useRef } from "react";
import { useAuth } from "@/lib/auth-context";
import { api, BusinessView } from "@/lib/api";
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
    </div>
  );
}
