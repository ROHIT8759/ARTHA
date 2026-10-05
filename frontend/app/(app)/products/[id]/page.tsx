"use client";

import { useEffect, useState } from "react";
import { useParams, useRouter } from "next/navigation";
import { api, ApiError, ProductView } from "@/lib/api";
import ProductForm, { ProductFormData } from "@/components/products/ProductForm";
import LoadingState from "@/components/ui/LoadingState";
import ErrorState from "@/components/ui/ErrorState";

export default function EditProductPage() {
  const params = useParams();
  const router = useRouter();
  const productId = params.id as string;
  
  const [product, setProduct] = useState<ProductView | null>(null);
  const [loading, setLoading] = useState(true);
  const [fetchError, setFetchError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    let mounted = true;
    const fetchProduct = async () => {
      try {
        const data = await api.getProduct(productId);
        if (mounted) setProduct(data);
      } catch (err: unknown) {
        if (mounted) setFetchError(err instanceof Error ? err.message : "Failed to load product");
      } finally {
        if (mounted) setLoading(false);
      }
    };
    fetchProduct();
    return () => { mounted = false; };
  }, [productId]);

  const handleSubmit = async (data: ProductFormData) => {
    setSubmitting(true);
    setSaveError(null);
    try {
      await api.updateProduct(productId, data as unknown as Parameters<typeof api.updateProduct>[1]);
      router.push("/products");
    } catch (err: unknown) {
      setSaveError(err instanceof ApiError ? err.message : err instanceof Error ? err.message : "An unexpected error occurred");
      setSubmitting(false);
    }
  };

  const toggleStatus = async () => {
    if (!product) return;
    setSubmitting(true);
    setSaveError(null);
    const newStatus = product.status === "active" ? "archived" : "active";
    try {
      const updated = await api.updateProduct(productId, { status: newStatus });
      setProduct(updated);
    } catch (err: unknown) {
      setSaveError(err instanceof Error ? err.message : "Failed to change status");
    } finally {
      setSubmitting(false);
    }
  };

  if (loading) return <LoadingState />;
  if (fetchError) return <ErrorState message={fetchError} onRetry={() => window.location.reload()} />;
  if (!product) return <ErrorState message="Product not found" />;

  return (
    <div>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "2rem", maxWidth: 600 }}>
        <h1 style={{ margin: 0 }}>Edit Product</h1>
        <button 
          onClick={toggleStatus} 
          disabled={submitting}
          className={product.status === "active" ? "btn btn-secondary" : "btn"}
          style={{ borderColor: product.status === "active" ? "var(--danger)" : "var(--accent)", color: product.status === "active" ? "var(--danger)" : "inherit" }}
        >
          {product.status === "active" ? "Deactivate" : "Activate"}
        </button>
      </div>
      
      <ProductForm 
        initialData={product}
        onSubmit={handleSubmit} 
        onCancel={() => router.push("/products")} 
        submitting={submitting} 
        error={saveError} 
      />
    </div>
  );
}
