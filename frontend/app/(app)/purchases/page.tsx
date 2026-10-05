"use client";

import EmptyState from "@/components/ui/EmptyState";

export default function PurchasesPage() {
  return (
    <div>
      <h1>Purchases</h1>
      <p className="muted" style={{ marginBottom: "2rem" }}>Manage vendor invoices and stock intake</p>
      
      <EmptyState 
        title="Purchases not implemented yet" 
        description="This section will allow you to record vendor purchases and scan OCR bills. It will be built in a future phase."
      />
    </div>
  );
}
