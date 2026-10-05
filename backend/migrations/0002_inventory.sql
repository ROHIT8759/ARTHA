CREATE TABLE IF NOT EXISTS inventory_transactions (
    transaction_id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL REFERENCES businesses(business_id),
    product_id TEXT NOT NULL REFERENCES products(product_id),
    transaction_type TEXT NOT NULL CHECK (transaction_type IN ('opening', 'purchase', 'sale', 'customer_return', 'supplier_return', 'adjustment', 'correction')),
    quantity_milli INTEGER NOT NULL,
    reference_id TEXT,
    reason TEXT,
    created_by TEXT NOT NULL REFERENCES users(user_id),
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_inventory_product ON inventory_transactions(business_id, product_id, created_at);
