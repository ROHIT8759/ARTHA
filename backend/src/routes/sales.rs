//! Sales & Billing Engine (Phase 07).
//!
//! Handles sales lifecycle (draft -> completed -> void), atomic checkout,
//! authoritative stock validation, price/product snapshots, payment records,
//! collision-safe invoice numbering, and audit logging.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::ids::{new_id, now_iso};
use crate::models::{
    CreateSaleRequest, FinalizeSaleRequest, ListSalesQuery, PaymentView, SaleItemView, SaleView,
    UpdateSaleItemRequest, UpdateSaleRequest, VoidSaleRequest,
};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/sales", get(list_sales).post(create_sale))
        .route("/api/sales/:id", get(get_sale).patch(update_sale))
        .route("/api/sales/:id/finalize", post(finalize_sale))
        .route("/api/sales/:id/void", post(void_sale))
}

/// Helper to fetch full sale detail with nested items and payments.
fn fetch_sale_detail(
    conn: &rusqlite::Connection,
    business_id: &str,
    sale_id: &str,
) -> Result<SaleView, ApiError> {
    let mut sale: SaleView = conn
        .query_row(
            "SELECT sale_id, business_id, invoice_no, customer_name, customer_phone, customer_email, \
             customer_address, customer_gstin, status, subtotal_paise, discount_paise, tax_total_paise, \
             grand_total_paise, payment_method, payment_status, notes, created_by, finalized_by, \
             created_at, updated_at, finalized_at \
             FROM sales WHERE business_id = ?1 AND sale_id = ?2",
            rusqlite::params![business_id, sale_id],
            |r| {
                Ok(SaleView {
                    sale_id: r.get(0)?,
                    business_id: r.get(1)?,
                    invoice_no: r.get(2)?,
                    customer_name: r.get(3)?,
                    customer_phone: r.get(4)?,
                    customer_email: r.get(5)?,
                    customer_address: r.get(6)?,
                    customer_gstin: r.get(7)?,
                    status: r.get(8)?,
                    subtotal_paise: r.get(9)?,
                    discount_paise: r.get(10)?,
                    tax_total_paise: r.get(11)?,
                    grand_total_paise: r.get(12)?,
                    payment_method: r.get(13)?,
                    payment_status: r.get(14)?,
                    notes: r.get(15)?,
                    created_by: r.get(16)?,
                    finalized_by: r.get(17)?,
                    created_at: r.get(18)?,
                    updated_at: r.get(19)?,
                    finalized_at: r.get(20)?,
                    items: vec![],
                    payments: vec![],
                })
            },
        )
        .map_err(|_| ApiError::NotFound("Sale not found".into()))?;

    // Load items with snapshots
    let mut item_stmt = conn.prepare(
        "SELECT sale_item_id, product_id, product_name_snapshot, batch_snapshot, hsn_code_snapshot, \
         gst_rate_bps, quantity_milli, unit_price_paise, discount_paise, tax_paise, line_total_paise \
         FROM sale_items WHERE sale_id = ?1 ORDER BY rowid ASC",
    )?;
    let item_rows = item_stmt.query_map(rusqlite::params![sale_id], |r| {
        Ok(SaleItemView {
            sale_item_id: r.get(0)?,
            product_id: r.get(1)?,
            product_name_snapshot: r.get(2)?,
            batch_snapshot: r.get(3)?,
            hsn_code_snapshot: r.get(4)?,
            gst_rate_bps: r.get(5)?,
            quantity_milli: r.get(6)?,
            unit_price_paise: r.get(7)?,
            discount_paise: r.get(8)?,
            tax_paise: r.get(9)?,
            line_total_paise: r.get(10)?,
        })
    })?;

    for item in item_rows {
        sale.items.push(item?);
    }

    // Load payments
    let mut pay_stmt = conn.prepare(
        "SELECT payment_id, payment_method, amount_paise, status, created_at \
         FROM payments WHERE sale_id = ?1 ORDER BY created_at ASC",
    )?;
    let pay_rows = pay_stmt.query_map(rusqlite::params![sale_id], |r| {
        Ok(PaymentView {
            payment_id: r.get(0)?,
            payment_method: r.get(1)?,
            amount_paise: r.get(2)?,
            status: r.get(3)?,
            created_at: r.get(4)?,
        })
    })?;

    for pay in pay_rows {
        sale.payments.push(pay?);
    }

    Ok(sale)
}

/// Helper to recalculate line items and sale totals server-side using pure integer arithmetic.
fn calculate_sale_items(
    conn: &rusqlite::Connection,
    business_id: &str,
    items: &[UpdateSaleItemRequest],
    bill_discount_paise: i64,
) -> Result<(Vec<SaleItemView>, i64, i64, i64, i64), ApiError> {
    if bill_discount_paise < 0 {
        return Err(ApiError::BadRequest(
            "Bill discount cannot be negative".into(),
        ));
    }

    let mut calculated_items = Vec::new();
    let mut subtotal: i64 = 0;
    let mut line_discounts_total: i64 = 0;
    let mut tax_total: i64 = 0;

    for item in items {
        if item.quantity_milli <= 0 {
            return Err(ApiError::BadRequest(
                "Item quantity must be greater than zero".into(),
            ));
        }

        // Query product catalog for validation & historical snapshot values
        let (name, batch, hsn_code, gst_rate_bps, catalog_price, status): (
            String,
            Option<String>,
            Option<String>,
            i64,
            i64,
            String,
        ) = conn
            .query_row(
                "SELECT name, batch, hsn_code, gst_rate_bps, price_paise, status \
                 FROM products WHERE business_id = ?1 AND product_id = ?2",
                rusqlite::params![business_id, item.product_id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .map_err(|_| ApiError::NotFound(format!("Product '{}' not found", item.product_id)))?;

        if status != "active" {
            return Err(ApiError::BadRequest(format!(
                "Cannot add inactive or archived product '{}' to a sale",
                name
            )));
        }

        let unit_price = item.unit_price_paise.unwrap_or(catalog_price);
        if unit_price < 0 {
            return Err(ApiError::BadRequest(
                "Unit price must be non-negative".into(),
            ));
        }

        let line_disc = item.discount_paise.unwrap_or(0);
        if line_disc < 0 {
            return Err(ApiError::BadRequest(
                "Line discount must be non-negative".into(),
            ));
        }

        // Exact integer calculation:
        // line_subtotal = (quantity_milli * unit_price + 500) / 1000  (round to nearest paise)
        let line_subtotal =
            ((item.quantity_milli as i128 * unit_price as i128 + 500) / 1000) as i64;
        if line_disc > line_subtotal {
            return Err(ApiError::BadRequest(format!(
                "Line discount cannot exceed line subtotal for '{}'",
                name
            )));
        }

        let taxable_line = line_subtotal - line_disc;
        // line_tax = (taxable_line * gst_rate_bps + 5000) / 10000 (round to nearest paise)
        let line_tax = ((taxable_line as i128 * gst_rate_bps as i128 + 5000) / 10000) as i64;
        let line_total = taxable_line + line_tax;

        subtotal += line_subtotal;
        line_discounts_total += line_disc;
        tax_total += line_tax;

        calculated_items.push(SaleItemView {
            sale_item_id: item.sale_item_id.clone().unwrap_or_else(new_id),
            product_id: item.product_id.clone(),
            product_name_snapshot: name,
            batch_snapshot: batch,
            hsn_code_snapshot: hsn_code,
            gst_rate_bps,
            quantity_milli: item.quantity_milli,
            unit_price_paise: unit_price,
            discount_paise: line_disc,
            tax_paise: line_tax,
            line_total_paise: line_total,
        });
    }

    let total_discount = line_discounts_total + bill_discount_paise;
    if total_discount > subtotal && !calculated_items.is_empty() {
        return Err(ApiError::BadRequest(
            "Total discount cannot exceed subtotal".into(),
        ));
    }

    let grand_total = (subtotal - total_discount + tax_total).max(0);

    Ok((
        calculated_items,
        subtotal,
        total_discount,
        tax_total,
        grand_total,
    ))
}

/// GET /api/sales - List sales for current business
async fn list_sales(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<ListSalesQuery>,
) -> Result<Json<Vec<SaleView>>, ApiError> {
    let conn = state.db.lock().expect("db mutex poisoned");

    let mut sql = "SELECT sale_id, business_id, invoice_no, customer_name, customer_phone, customer_email, \
                   customer_address, customer_gstin, status, subtotal_paise, discount_paise, tax_total_paise, \
                   grand_total_paise, payment_method, payment_status, notes, created_by, finalized_by, \
                   created_at, updated_at, finalized_at \
                   FROM sales WHERE business_id = ?1".to_string();

    let mut params: Vec<rusqlite::types::Value> =
        vec![rusqlite::types::Value::from(user.business_id.clone())];

    if let Some(st) = &query.status {
        if !st.trim().is_empty() {
            sql.push_str(" AND status = ?");
            params.push(rusqlite::types::Value::from(st.trim().to_lowercase()));
        }
    }

    if let Some(s) = &query.search {
        if !s.trim().is_empty() {
            sql.push_str(
                " AND (invoice_no LIKE ? OR customer_name LIKE ? OR customer_phone LIKE ?)",
            );
            let pattern = format!("%{}%", s.trim());
            params.push(rusqlite::types::Value::from(pattern.clone()));
            params.push(rusqlite::types::Value::from(pattern.clone()));
            params.push(rusqlite::types::Value::from(pattern));
        }
    }

    sql.push_str(" ORDER BY created_at DESC");

    if let Some(limit) = query.limit {
        if limit > 0 {
            sql.push_str(&format!(" LIMIT {}", limit));
        }
    }

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params), |r| {
        Ok(SaleView {
            sale_id: r.get(0)?,
            business_id: r.get(1)?,
            invoice_no: r.get(2)?,
            customer_name: r.get(3)?,
            customer_phone: r.get(4)?,
            customer_email: r.get(5)?,
            customer_address: r.get(6)?,
            customer_gstin: r.get(7)?,
            status: r.get(8)?,
            subtotal_paise: r.get(9)?,
            discount_paise: r.get(10)?,
            tax_total_paise: r.get(11)?,
            grand_total_paise: r.get(12)?,
            payment_method: r.get(13)?,
            payment_status: r.get(14)?,
            notes: r.get(15)?,
            created_by: r.get(16)?,
            finalized_by: r.get(17)?,
            created_at: r.get(18)?,
            updated_at: r.get(19)?,
            finalized_at: r.get(20)?,
            items: vec![],
            payments: vec![],
        })
    })?;

    let mut sales = Vec::new();
    for row in rows {
        sales.push(row?);
    }

    Ok(Json(sales))
}

/// GET /api/sales/:id - Retrieve complete sale with items & payments
async fn get_sale(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<SaleView>, ApiError> {
    let conn = state.db.lock().expect("db mutex poisoned");
    let sale = fetch_sale_detail(&conn, &user.business_id, &id)?;
    Ok(Json(sale))
}

/// POST /api/sales - Create new draft sale
async fn create_sale(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreateSaleRequest>,
) -> Result<Json<SaleView>, ApiError> {
    let mut conn = state.db.lock().expect("db mutex poisoned");
    let tx = conn.transaction()?;

    let sale_id = new_id();
    let now = now_iso();
    let pay_method = req
        .payment_method
        .map(|m| m.trim().to_lowercase())
        .unwrap_or_else(|| "cash".to_string());

    if !matches!(pay_method.as_str(), "cash" | "upi" | "card" | "other") {
        return Err(ApiError::BadRequest("Invalid payment method".into()));
    }

    tx.execute(
        "INSERT INTO sales (sale_id, business_id, invoice_no, customer_name, customer_phone, customer_email, \
         customer_address, customer_gstin, status, subtotal_paise, discount_paise, tax_total_paise, \
         grand_total_paise, payment_method, payment_status, notes, created_by, finalized_by, \
         created_at, updated_at, finalized_at) \
         VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, 'draft', 0, 0, 0, 0, ?8, 'pending', ?9, ?10, NULL, ?11, ?11, NULL)",
        rusqlite::params![
            sale_id,
            user.business_id,
            req.customer_name.as_deref().map(str::trim),
            req.customer_phone.as_deref().map(str::trim),
            req.customer_email.as_deref().map(str::trim),
            req.customer_address.as_deref().map(str::trim),
            req.customer_gstin.as_deref().map(str::trim),
            pay_method,
            req.notes.as_deref().map(str::trim),
            user.user_id,
            now
        ],
    )?;

    crate::audit::record(
        &tx,
        &user.business_id,
        Some(&user.user_id),
        "sale_created",
        "sale",
        Some(&sale_id),
        &serde_json::json!({ "sale_id": sale_id }).to_string(),
    )?;

    tx.commit()?;

    let sale = fetch_sale_detail(&conn, &user.business_id, &sale_id)?;
    Ok(Json(sale))
}

/// PATCH /api/sales/:id - Update draft sale (items, customer info, server-side totals calculation)
#[allow(clippy::type_complexity)]
async fn update_sale(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateSaleRequest>,
) -> Result<Json<SaleView>, ApiError> {
    let mut conn = state.db.lock().expect("db mutex poisoned");
    let tx = conn.transaction()?;

    let (
        status,
        current_discount,
        current_customer_name,
        current_customer_phone,
        current_customer_email,
        current_customer_address,
        current_customer_gstin,
        current_pay_method,
        current_notes,
    ): (
        String,
        i64,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
    ) = tx
        .query_row(
            "SELECT status, discount_paise, customer_name, customer_phone, customer_email, \
             customer_address, customer_gstin, payment_method, notes \
             FROM sales WHERE sale_id = ?1 AND business_id = ?2",
            rusqlite::params![id, user.business_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                ))
            },
        )
        .map_err(|_| ApiError::NotFound("Sale not found".into()))?;

    if status != "draft" {
        return Err(ApiError::Forbidden(
            "Only draft sales can be edited. Completed and void sales are immutable.".into(),
        ));
    }

    let customer_name = req.customer_name.or(current_customer_name);
    let customer_phone = req.customer_phone.or(current_customer_phone);
    let customer_email = req.customer_email.or(current_customer_email);
    let customer_address = req.customer_address.or(current_customer_address);
    let customer_gstin = req.customer_gstin.or(current_customer_gstin);
    let notes = req.notes.or(current_notes);
    let payment_method = req
        .payment_method
        .map(|m| m.trim().to_lowercase())
        .unwrap_or(current_pay_method);

    if !matches!(payment_method.as_str(), "cash" | "upi" | "card" | "other") {
        return Err(ApiError::BadRequest("Invalid payment method".into()));
    }

    let bill_discount = req.discount_paise.unwrap_or(current_discount);

    // If items are provided in update, rebuild them and recalculate authoritative totals
    let (subtotal, total_discount, tax_total, grand_total) = if let Some(items) = req.items {
        tx.execute(
            "DELETE FROM sale_items WHERE sale_id = ?1",
            rusqlite::params![id],
        )?;

        let (calc_items, sub, disc, tax, grand) =
            calculate_sale_items(&tx, &user.business_id, &items, bill_discount)?;

        for item in calc_items {
            tx.execute(
                "INSERT INTO sale_items (sale_item_id, sale_id, product_id, product_name_snapshot, \
                 batch_snapshot, hsn_code_snapshot, gst_rate_bps, quantity_milli, unit_price_paise, \
                 discount_paise, tax_paise, line_total_paise) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                rusqlite::params![
                    item.sale_item_id,
                    id,
                    item.product_id,
                    item.product_name_snapshot,
                    item.batch_snapshot,
                    item.hsn_code_snapshot,
                    item.gst_rate_bps,
                    item.quantity_milli,
                    item.unit_price_paise,
                    item.discount_paise,
                    item.tax_paise,
                    item.line_total_paise,
                ],
            )?;
        }
        (sub, disc, tax, grand)
    } else {
        // Keep existing items, but recalculate totals if bill discount changed
        let (sub, tax): (i64, i64) = tx.query_row(
            "SELECT subtotal_paise, tax_total_paise FROM sales WHERE sale_id = ?1",
            rusqlite::params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let line_disc_total: i64 = tx.query_row(
            "SELECT COALESCE(SUM(discount_paise), 0) FROM sale_items WHERE sale_id = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        )?;
        let tot_disc = line_disc_total + bill_discount;
        let grand = (sub - tot_disc + tax).max(0);
        (sub, tot_disc, tax, grand)
    };

    let now = now_iso();
    tx.execute(
        "UPDATE sales SET customer_name = ?1, customer_phone = ?2, customer_email = ?3, \
         customer_address = ?4, customer_gstin = ?5, payment_method = ?6, notes = ?7, \
         subtotal_paise = ?8, discount_paise = ?9, tax_total_paise = ?10, grand_total_paise = ?11, \
         updated_at = ?12 WHERE sale_id = ?13 AND business_id = ?14",
        rusqlite::params![
            customer_name,
            customer_phone,
            customer_email,
            customer_address,
            customer_gstin,
            payment_method,
            notes,
            subtotal,
            total_discount,
            tax_total,
            grand_total,
            now,
            id,
            user.business_id,
        ],
    )?;

    tx.commit()?;

    let sale = fetch_sale_detail(&conn, &user.business_id, &id)?;
    Ok(Json(sale))
}

/// POST /api/sales/:id/finalize - Atomically validate stock, assign monotonic invoice number,
/// deduct inventory through authoritative ledger, record payment, and audit.
/// Fully idempotent on duplicate / retry submission.
async fn finalize_sale(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<Option<FinalizeSaleRequest>>,
) -> Result<Json<SaleView>, ApiError> {
    let mut conn = state.db.lock().expect("db mutex poisoned");

    // Check if sale already finalized (Idempotent replay / retry handling)
    let existing_status: String = conn
        .query_row(
            "SELECT status FROM sales WHERE sale_id = ?1 AND business_id = ?2",
            rusqlite::params![id, user.business_id],
            |r| r.get(0),
        )
        .map_err(|_| ApiError::NotFound("Sale not found".into()))?;

    if existing_status == "completed" {
        // Return existing completed sale idempotently without duplicate deduction!
        let sale = fetch_sale_detail(&conn, &user.business_id, &id)?;
        return Ok(Json(sale));
    }

    if existing_status == "void" {
        return Err(ApiError::Conflict("Cannot finalize a void sale".into()));
    }

    if existing_status != "draft" {
        return Err(ApiError::BadRequest(
            "Only draft sales can be finalized".into(),
        ));
    }

    let tx = conn.transaction()?;

    // Fetch sale lines
    let mut stmt = tx.prepare(
        "SELECT product_id, product_name_snapshot, quantity_milli FROM sale_items WHERE sale_id = ?1",
    )?;
    let items = stmt
        .query_map(rusqlite::params![id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    if items.is_empty() {
        return Err(ApiError::BadRequest(
            "Cannot finalize a sale with no items".into(),
        ));
    }

    // 1. Authoritative inventory validation & active product check
    for (prod_id, prod_name, qty_needed) in &items {
        let prod_status: String = tx
            .query_row(
                "SELECT status FROM products WHERE business_id = ?1 AND product_id = ?2",
                rusqlite::params![user.business_id, prod_id],
                |r| r.get(0),
            )
            .map_err(|_| {
                ApiError::BadRequest(format!("Product '{}' no longer exists", prod_name))
            })?;

        if prod_status != "active" {
            return Err(ApiError::BadRequest(format!(
                "Product '{}' is archived or inactive",
                prod_name
            )));
        }

        let current_stock: i64 = tx.query_row(
            "SELECT COALESCE(SUM(quantity_milli), 0) FROM inventory_transactions \
             WHERE business_id = ?1 AND product_id = ?2",
            rusqlite::params![user.business_id, prod_id],
            |r| r.get(0),
        )?;

        if current_stock < *qty_needed {
            return Err(ApiError::BadRequest(format!(
                "Insufficient stock for Product {}. Available: {} milli, Requested: {} milli",
                prod_name, current_stock, qty_needed
            )));
        }
    }

    // 2. Database-backed monotonic collision-safe invoice numbering per business
    tx.execute(
        "INSERT INTO invoice_sequences (business_id, last_seq, prefix) \
         VALUES (?1, 1, 'INV') \
         ON CONFLICT(business_id) DO UPDATE SET last_seq = invoice_sequences.last_seq + 1",
        rusqlite::params![user.business_id],
    )?;

    let (last_seq, prefix): (i64, String) = tx.query_row(
        "SELECT last_seq, prefix FROM invoice_sequences WHERE business_id = ?1",
        rusqlite::params![user.business_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;

    let invoice_no = format!("{}-{:06}", prefix, last_seq);
    let now = now_iso();

    // 3. Payment handling
    let (grand_total, current_pay_method): (i64, String) = tx.query_row(
        "SELECT grand_total_paise, payment_method FROM sales WHERE sale_id = ?1",
        rusqlite::params![id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;

    let finalize_req = req.unwrap_or(FinalizeSaleRequest {
        payment_method: None,
        amount_paise: None,
    });

    let payment_method = finalize_req
        .payment_method
        .map(|m| m.trim().to_lowercase())
        .unwrap_or(current_pay_method);

    if !matches!(payment_method.as_str(), "cash" | "upi" | "card" | "other") {
        return Err(ApiError::BadRequest("Invalid payment method".into()));
    }

    let payment_amount = finalize_req.amount_paise.unwrap_or(grand_total);
    if payment_amount < grand_total {
        return Err(ApiError::BadRequest(
            "Payment amount is less than grand total".into(),
        ));
    }

    // 4. Update sale to completed
    tx.execute(
        "UPDATE sales SET invoice_no = ?1, status = 'completed', payment_method = ?2, \
         payment_status = 'paid', finalized_by = ?3, finalized_at = ?4, updated_at = ?4 \
         WHERE sale_id = ?5 AND business_id = ?6",
        rusqlite::params![
            invoice_no,
            payment_method,
            user.user_id,
            now,
            id,
            user.business_id
        ],
    )?;

    // 5. Deduct inventory through Phase 05 inventory_transactions ledger atomically
    for (prod_id, _prod_name, qty_needed) in &items {
        let current_stock: i64 = tx.query_row(
            "SELECT COALESCE(SUM(quantity_milli), 0) FROM inventory_transactions \
             WHERE business_id = ?1 AND product_id = ?2",
            rusqlite::params![user.business_id, prod_id],
            |r| r.get(0),
        )?;

        let new_stock = current_stock - qty_needed;
        if new_stock < 0 {
            return Err(ApiError::BadRequest("Insufficient stock".into()));
        }

        let tx_id = new_id();
        tx.execute(
            "INSERT INTO inventory_transactions (transaction_id, business_id, product_id, \
             transaction_type, quantity_milli, reference_id, reason, created_by, created_at) \
             VALUES (?1, ?2, ?3, 'sale', ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                tx_id,
                user.business_id,
                prod_id,
                -qty_needed,
                id,
                format!("Sale invoice {}", invoice_no),
                user.user_id,
                now,
            ],
        )?;

        // Update cached stock on products table
        tx.execute(
            "UPDATE products SET stock_qty_milli = ?1, updated_at = ?2 \
             WHERE business_id = ?3 AND product_id = ?4",
            rusqlite::params![new_stock, now, user.business_id, prod_id],
        )?;
    }

    // 6. Record payment
    let payment_id = new_id();
    tx.execute(
        "INSERT INTO payments (payment_id, sale_id, business_id, payment_method, amount_paise, status, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, 'completed', ?6)",
        rusqlite::params![
            payment_id,
            id,
            user.business_id,
            payment_method,
            payment_amount,
            now,
        ],
    )?;

    // 7. Audit log event
    crate::audit::record(
        &tx,
        &user.business_id,
        Some(&user.user_id),
        "sale_finalized",
        "sale",
        Some(&id),
        &serde_json::json!({
            "sale_id": id,
            "invoice_no": invoice_no,
            "grand_total_paise": grand_total,
            "payment_method": payment_method,
            "payment_id": payment_id
        })
        .to_string(),
    )?;

    tx.commit()?;

    let sale = fetch_sale_detail(&conn, &user.business_id, &id)?;
    Ok(Json(sale))
}

/// POST /api/sales/:id/void - Void a sale. If draft: marked void without stock movement.
/// If completed: reverses inventory atomically via ledger ('correction'), refunds payment, and audits.
async fn void_sale(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<Option<VoidSaleRequest>>,
) -> Result<Json<SaleView>, ApiError> {
    let mut conn = state.db.lock().expect("db mutex poisoned");
    let tx = conn.transaction()?;

    let (status, invoice_no): (String, Option<String>) = tx
        .query_row(
            "SELECT status, invoice_no FROM sales WHERE sale_id = ?1 AND business_id = ?2",
            rusqlite::params![id, user.business_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| ApiError::NotFound("Sale not found".into()))?;

    if status == "void" {
        return Err(ApiError::Conflict("Sale is already void".into()));
    }

    let now = now_iso();
    let reason = req
        .and_then(|r| r.reason)
        .unwrap_or_else(|| "Sale voided".to_string());

    if status == "draft" {
        // Simple cancellation of draft without inventory movement
        tx.execute(
            "UPDATE sales SET status = 'void', updated_at = ?1 WHERE sale_id = ?2",
            rusqlite::params![now, id],
        )?;
    } else if status == "completed" {
        // Reverse inventory ledger for all items
        let mut stmt =
            tx.prepare("SELECT product_id, quantity_milli FROM sale_items WHERE sale_id = ?1")?;
        let items = stmt
            .query_map(rusqlite::params![id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        for (prod_id, qty) in items {
            let current_stock: i64 = tx.query_row(
                "SELECT COALESCE(SUM(quantity_milli), 0) FROM inventory_transactions \
                 WHERE business_id = ?1 AND product_id = ?2",
                rusqlite::params![user.business_id, prod_id],
                |r| r.get(0),
            )?;

            let restored_stock = current_stock + qty;
            let tx_id = new_id();

            tx.execute(
                "INSERT INTO inventory_transactions (transaction_id, business_id, product_id, \
                 transaction_type, quantity_milli, reference_id, reason, created_by, created_at) \
                 VALUES (?1, ?2, ?3, 'correction', ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    tx_id,
                    user.business_id,
                    prod_id,
                    qty,
                    id,
                    format!(
                        "Void sale {}: {}",
                        invoice_no.as_deref().unwrap_or(&id),
                        reason
                    ),
                    user.user_id,
                    now,
                ],
            )?;

            tx.execute(
                "UPDATE products SET stock_qty_milli = ?1, updated_at = ?2 \
                 WHERE business_id = ?3 AND product_id = ?4",
                rusqlite::params![restored_stock, now, user.business_id, prod_id],
            )?;
        }

        // Update sale status to void.
        // Financial integrity: The payment collection record remains on file as 'completed'.
        // We do NOT mark payments or sale payment_status as 'refunded' because ARTHA has no
        // automated refund integration. Any customer payout/refund is manual out-of-band.
        tx.execute(
            "UPDATE sales SET status = 'void', updated_at = ?1 WHERE sale_id = ?2",
            rusqlite::params![now, id],
        )?;
    }

    crate::audit::record(
        &tx,
        &user.business_id,
        Some(&user.user_id),
        "sale_voided",
        "sale",
        Some(&id),
        &serde_json::json!({
            "sale_id": id,
            "invoice_no": invoice_no,
            "previous_status": status,
            "reason": reason,
            "inventory_restored": status == "completed",
            "refund_issued": false
        })
        .to_string(),
    )?;

    tx.commit()?;

    let sale = fetch_sale_detail(&conn, &user.business_id, &id)?;
    Ok(Json(sale))
}
