use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct SystemFunction {
    pub id: i64,
    #[serde(rename = "clusterId")]
    pub cluster_id: i64,
    #[serde(rename = "categoryName")]
    pub category_name: String,
    #[serde(rename = "functionName")]
    pub function_name: String,
    pub description: String,
    #[serde(rename = "sqlQuery")]
    pub sql_query: String,
    #[serde(rename = "displayOrder")]
    pub display_order: i32,
    #[serde(rename = "categoryOrder")]
    pub category_order: i32,
    #[serde(rename = "isFavorited")]
    pub is_favorited: bool,
    #[serde(rename = "isSystem")]
    pub is_system: bool,
    #[serde(rename = "createdBy")]
    pub created_by: i64,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "updatedAt")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateFunctionRequest {
    #[validate(length(min = 1, max = 100))]
    pub category_name: String,
    #[validate(length(min = 1, max = 100))]
    pub function_name: String,
    #[validate(length(min = 1, max = 500))]
    pub description: String,
    #[validate(length(min = 1))]
    pub sql_query: String,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct UpdateFunctionRequest {
    #[validate(length(min = 1, max = 100))]
    pub category_name: String,
    #[validate(length(min = 1, max = 100))]
    pub function_name: String,
    #[validate(length(min = 1, max = 500))]
    pub description: String,
    #[validate(length(min = 1))]
    pub sql_query: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateOrderRequest {
    pub functions: Vec<FunctionOrder>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct FunctionOrder {
    pub id: i64,
    #[serde(rename = "displayOrder")]
    pub display_order: i32,
    #[serde(rename = "categoryOrder")]
    pub category_order: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct SystemFunctionPreference {
    pub id: i64,
    #[serde(rename = "clusterId")]
    pub cluster_id: i64,
    #[serde(rename = "functionId")]
    pub function_id: i64,
    #[serde(rename = "categoryOrder")]
    pub category_order: i32,
    #[serde(rename = "displayOrder")]
    pub display_order: i32,
    #[serde(rename = "isFavorited")]
    pub is_favorited: bool,
    #[serde(rename = "updatedAt")]
    pub updated_at: DateTime<Utc>,
}
