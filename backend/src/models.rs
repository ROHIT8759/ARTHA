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
