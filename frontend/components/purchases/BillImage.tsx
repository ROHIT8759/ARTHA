"use client";

import { useEffect, useState } from "react";
import { api } from "@/lib/api";

interface BillImageProps {
  purchaseId: string;
  /** Changes when a new image is attached, which triggers a re-fetch. */
  imageId: string;
}

type Loaded = { key: string; url?: string; error?: string };

/**
 * Renders the attached purchase bill photo.
 *
 * The image endpoint is authenticated with an `Authorization: Bearer` header,
 * which a plain `<img src>` request cannot send (the browser would get a 401
 * and show a broken image). The bytes are therefore fetched with the session
 * token and displayed through an object URL, which is revoked on cleanup.
 */
export default function BillImage({ purchaseId, imageId }: BillImageProps) {
  const [attempt, setAttempt] = useState(0);
  const [result, setResult] = useState<Loaded | null>(null);
  const key = `${purchaseId}:${imageId}:${attempt}`;

  useEffect(() => {
    let cancelled = false;
    let objectUrl: string | undefined;
    api
      .fetchBillImageBlob(purchaseId)
      .then((blob) => {
        if (cancelled) return;
        objectUrl = URL.createObjectURL(blob);
        setResult({ key, url: objectUrl });
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setResult({ key, error: err instanceof Error ? err.message : "Failed to load bill image" });
      });
    return () => {
      cancelled = true;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [purchaseId, imageId, attempt, key]);

  const current = result && result.key === key ? result : null;

  if (!current) {
    return (
      <p className="muted" role="status" style={{ margin: 0 }}>
        Loading bill photo...
      </p>
    );
  }

  if (current.error || !current.url) {
    return (
      <div role="alert" style={{ textAlign: "center", padding: "1rem" }}>
        <p style={{ color: "var(--danger)", margin: "0 0 0.75rem" }}>
          Could not load the bill photo: {current.error ?? "unknown error"}
        </p>
        <button className="btn btn-secondary" onClick={() => setAttempt((n) => n + 1)}>
          Retry
        </button>
      </div>
    );
  }

  return (
    // eslint-disable-next-line @next/next/no-img-element
    <img
      src={current.url}
      alt="Original Bill"
      style={{ maxWidth: "100%", height: "auto", display: "block" }}
    />
  );
}
