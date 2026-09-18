//! Read-only diagnostic tool registry (default tools of the OLAP ops agent).

pub mod audit;
pub mod capacity;
pub mod explain;
pub mod metrics;
pub mod nodes;
pub mod profile;
pub mod propose;
pub mod queries;
pub mod variables;
pub mod web;

use std::sync::Arc;

use stellar_macros::app_db;

use crate::db::AppDb;

use super::tool::{AgentTool, ToolContext};

/// Build the default read-only tool set for one request.
#[app_db]
pub fn create_tools<DB: AppDb>(ctx: Arc<ToolContext<DB>>) -> Vec<Box<dyn AgentTool>> {
    vec![
        Box::new(metrics::QueryMetricsTool { ctx: Arc::clone(&ctx) }),
        Box::new(nodes::QueryNodesTool { ctx: Arc::clone(&ctx) }),
        Box::new(queries::QueryRunningQueriesTool { ctx: Arc::clone(&ctx) }),
        Box::new(audit::QuerySlowQueriesTool { ctx: Arc::clone(&ctx) }),
        Box::new(explain::QueryExplainTool { ctx: Arc::clone(&ctx) }),
        Box::new(profile::QueryProfileDiagnosticsTool { ctx: Arc::clone(&ctx) }),
        Box::new(variables::QueryVariablesTool { ctx: Arc::clone(&ctx) }),
        Box::new(capacity::QueryCapacityForecastTool { ctx: Arc::clone(&ctx) }),
        Box::new(web::SearchWebTool { ctx: Arc::clone(&ctx) }),
        Box::new(web::FetchDocTool { ctx: Arc::clone(&ctx) }),
        Box::new(propose::ProposeActionTool { ctx }),
    ]
}
