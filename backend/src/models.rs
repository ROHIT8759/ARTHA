//! Request/response and row types shared across route modules.
//!
//! Kept deliberately flat (no ORM) — V0 talks to `rusqlite` directly in each
//! route module, and these structs are just the (de)serialization contract
//! with the frontend.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Owner,
    Staff,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Staff => "staff",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Role> {
        match s {
            "owner" => Some(Role::Owner),
            "staff" => Some(Role::Staff),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct UserView {
    pub user_id: String,
    pub username: String,
    pub role: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct SetupBusinessRequest {
    pub business_name: String,
    pub owner_username: String,
    pub owner_password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserView,
    pub expires_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateStaffRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateProductRequest {
    pub name: String,
    pub batch: Option<String>,
    pub hsn_code: Option<String>,
    pub gst_rate_bps: i64,
    pub qr_code: String,
    pub price_paise: i64,
    pub cost_paise: i64,
    pub unit: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListProductsQuery {
    pub search: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProductRequest {
    pub name: Option<String>,
    pub batch: Option<String>,
    pub hsn_code: Option<String>,
    pub gst_rate_bps: Option<i64>,
    pub qr_code: Option<String>,
    pub price_paise: Option<i64>,
    pub cost_paise: Option<i64>,
    pub unit: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProductView {
    pub product_id: String,
    pub name: String,
    pub batch: Option<String>,
    pub hsn_code: Option<String>,
    pub gst_rate_bps: i64,
    pub qr_code: String,
    pub price_paise: i64,
    pub cost_paise: i64,
    pub stock_qty_milli: i64,
    pub unit: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaleItemView {
    pub sale_item_id: String,
    pub product_id: String,
    pub product_name_snapshot: String,
    pub batch_snapshot: Option<String>,
    pub hsn_code_snapshot: Option<String>,
    pub gst_rate_bps: i64,
    pub quantity_milli: i64,
    pub unit_price_paise: i64,
    pub discount_paise: i64,
    pub tax_paise: i64,
    pub line_total_paise: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentView {
    pub payment_id: String,
    pub payment_method: String,
    pub amount_paise: i64,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaleView {
    pub sale_id: String,
    pub business_id: String,
    pub invoice_no: Option<String>,
    pub customer_name: Option<String>,
    pub customer_phone: Option<String>,
    pub customer_email: Option<String>,
    pub customer_address: Option<String>,
    pub customer_gstin: Option<String>,
    pub status: String, // "draft", "completed", "void"
    pub subtotal_paise: i64,
    pub discount_paise: i64,
    pub tax_total_paise: i64,
    pub grand_total_paise: i64,
    pub payment_method: String,
    pub payment_status: String,
    pub notes: Option<String>,
    pub created_by: String,
    pub finalized_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub finalized_at: Option<String>,
    pub items: Vec<SaleItemView>,
    pub payments: Vec<PaymentView>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSaleRequest {
    pub customer_name: Option<String>,
    pub customer_phone: Option<String>,
    pub customer_email: Option<String>,
    pub customer_address: Option<String>,
    pub customer_gstin: Option<String>,
    pub payment_method: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSaleItemRequest {
    pub sale_item_id: Option<String>,
    pub product_id: String,
    pub quantity_milli: i64,
    pub unit_price_paise: Option<i64>,
    pub discount_paise: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSaleRequest {
    pub customer_name: Option<String>,
    pub customer_phone: Option<String>,
    pub customer_email: Option<String>,
    pub customer_address: Option<String>,
    pub customer_gstin: Option<String>,
    pub payment_method: Option<String>,
    pub discount_paise: Option<i64>,
    pub notes: Option<String>,
    pub items: Option<Vec<UpdateSaleItemRequest>>,
}

#[derive(Debug, Deserialize)]
pub struct FinalizeSaleRequest {
    pub payment_method: Option<String>,
    pub amount_paise: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct VoidSaleRequest {
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListSalesQuery {
    pub status: Option<String>,
    pub search: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BusinessSettingsView {
    pub registered_name: Option<String>,
    pub address: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub pincode: Option<String>,
    pub country: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub gstin: Option<String>,
    pub logo_data_url: Option<String>,
    pub invoice_footer: Option<String>,
    pub terms_and_conditions: Option<String>,
    pub default_layout: Option<String>,
    pub show_hsn: Option<bool>,
    pub show_tax_breakdown: Option<bool>,
    pub show_logo: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessView {
    pub business_id: String,
    pub name: String,
    pub settings: BusinessSettingsView,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateBusinessRequest {
    pub name: Option<String>,
    pub registered_name: Option<String>,
    pub address: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub pincode: Option<String>,
    pub country: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub gstin: Option<String>,
    pub invoice_footer: Option<String>,
    pub terms_and_conditions: Option<String>,
    pub default_layout: Option<String>,
    pub show_hsn: Option<bool>,
    pub show_tax_breakdown: Option<bool>,
    pub show_logo: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UploadLogoRequest {
    pub logo_data_url: String,
}
