-- ARTHA Phase 07: Sales, Billing & Payments Schema Migration

-- Recreate sales table with complete billing metadata, discounts, and payment tracking
CREATE TABLE sales_new (
    sale_id             TEXT PRIMARY KEY,
    business_id         TEXT NOT NULL REFERENCES businesses(business_id),
    invoice_no          TEXT,
    customer_name       TEXT,
    customer_phone      TEXT,
    customer_email      TEXT,
    customer_address    TEXT,
    customer_gstin      TEXT,
    status              TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'completed', 'void')),
    subtotal_paise      INTEGER NOT NULL DEFAULT 0,
    discount_paise      INTEGER NOT NULL DEFAULT 0,
    tax_total_paise     INTEGER NOT NULL DEFAULT 0,
    grand_total_paise   INTEGER NOT NULL DEFAULT 0,
    payment_method      TEXT NOT NULL DEFAULT 'cash' CHECK (payment_method IN ('cash', 'upi', 'card', 'other')),
    payment_status      TEXT NOT NULL DEFAULT 'pending' CHECK (payment_status IN ('pending', 'paid', 'refunded')),
    notes               TEXT,
    created_by          TEXT NOT NULL REFERENCES users(user_id),
    finalized_by        TEXT REFERENCES users(user_id),
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    finalized_at        TEXT
);

INSERT INTO sales_new (sale_id, business_id, invoice_no, customer_name, status, grand_total_paise, created_by, created_at, updated_at, finalized_at)
SELECT sale_id, business_id, invoice_no, customer_name, 
       CASE WHEN status = 'approved' THEN 'completed' ELSE status END,
       total_paise, created_by, created_at, updated_at, approved_at
FROM sales;

DROP TABLE sales;
ALTER TABLE sales_new RENAME TO sales;

CREATE INDEX idx_sales_business ON sales(business_id);
CREATE UNIQUE INDEX idx_sales_business_invoice ON sales(business_id, invoice_no) WHERE invoice_no IS NOT NULL;

-- Recreate sale_items with historical snapshots and tax details
CREATE TABLE sale_items_new (
    sale_item_id          TEXT PRIMARY KEY,
    sale_id               TEXT NOT NULL REFERENCES sales(sale_id),
    product_id            TEXT NOT NULL REFERENCES products(product_id),
    product_name_snapshot TEXT NOT NULL,
    batch_snapshot        TEXT,
    hsn_code_snapshot     TEXT,
    gst_rate_bps          INTEGER NOT NULL DEFAULT 0,
    quantity_milli        INTEGER NOT NULL,
    unit_price_paise      INTEGER NOT NULL,
    discount_paise        INTEGER NOT NULL DEFAULT 0,
    tax_paise             INTEGER NOT NULL DEFAULT 0,
    line_total_paise      INTEGER NOT NULL
);

INSERT INTO sale_items_new (sale_item_id, sale_id, product_id, product_name_snapshot, gst_rate_bps, quantity_milli, unit_price_paise, discount_paise, line_total_paise)
SELECT si.sale_item_id, si.sale_id, si.product_id, COALESCE(p.name, 'Unknown Product'), si.gst_rate_bps, si.quantity_milli, si.rate_paise, si.discount_paise, si.line_total_paise
FROM sale_items si
LEFT JOIN products p ON si.product_id = p.product_id;

DROP TABLE sale_items;
ALTER TABLE sale_items_new RENAME TO sale_items;

CREATE INDEX idx_sale_items_sale ON sale_items(sale_id);
CREATE INDEX idx_sale_items_product ON sale_items(product_id);

-- Payment records table
CREATE TABLE IF NOT EXISTS payments (
    payment_id     TEXT PRIMARY KEY,
    sale_id        TEXT NOT NULL REFERENCES sales(sale_id),
    business_id    TEXT NOT NULL REFERENCES businesses(business_id),
    payment_method TEXT NOT NULL CHECK (payment_method IN ('cash', 'upi', 'card', 'other')),
    amount_paise   INTEGER NOT NULL,
    status         TEXT NOT NULL CHECK (status IN ('completed', 'failed', 'refunded')),
    created_at     TEXT NOT NULL
);

CREATE INDEX idx_payments_sale ON payments(sale_id);
CREATE INDEX idx_payments_business ON payments(business_id);

-- Sequence table for atomic, monotonic, collision-safe invoice numbering per business
CREATE TABLE IF NOT EXISTS invoice_sequences (
    business_id TEXT PRIMARY KEY REFERENCES businesses(business_id),
    last_seq    INTEGER NOT NULL DEFAULT 0,
    prefix      TEXT NOT NULL DEFAULT 'INV'
);
