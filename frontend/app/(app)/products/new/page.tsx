"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";
import { api, ApiError } from "@/lib/api";
import ProductForm, { ProductFormData } from "@/components/products/ProductForm";

export default function NewProductPage() {
  const router = useRouter();
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (data: ProductFormData) => {
    setSubmitting(true);
    setError(null);
    try {
      await api.createProduct(data);
      router.push("/products");
    } catch (err: unknown) {
      setError(err instanceof ApiError ? err.message : err instanceof Error ? err.message : "An unexpected error occurred");
      setSubmitting(false);
    }
  };

  return (
    <div>
      <h1 style={{ marginBottom: "2rem" }}>New Product</h1>
      <ProductForm 
        onSubmit={handleSubmit} 
        onCancel={() => router.push("/products")} 
        submitting={submitting} 
        error={error} 
      />
    </div>
  );
}
