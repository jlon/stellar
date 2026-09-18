//! `query_capacity_forecast` -- 磁盘容量趋势预测（六阶段容量自适应）：近 24h 增长
//! 斜率 → 满盘 ETA，供对话场景直接回答"多久满盘/是否需扩容"。

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use stellar_macros::app_impl;

use crate::db::AppDb;
use crate::services::agent_runtime::capacity::forecast_disk;
use crate::services::ops_agent::tool::{AgentTool, ToolContext};

pub struct QueryCapacityForecastTool<DB: AppDb> {
    pub(crate) ctx: Arc<ToolContext<DB>>,
}

#[app_impl]
#[async_trait]
impl<DB: AppDb> AgentTool for QueryCapacityForecastTool<DB> {
    fn name(&self) -> &'static str {
        "query_capacity_forecast"
    }

    fn description(&self) -> &'static str {
        "查询磁盘容量趋势预测：基于近 24 小时快照线性回归，给出当前使用率、每日增长速率\
         与预计满盘天数（ETA）。用于判断是否需要扩容/清理，是容量规划的第一手证据。"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _args: Value) -> Result<String, String> {
        // 存算分离集群的数据在对象存储：本地盘只是数据缓存，缓存写满按 LRU 淘汰，
        // 拿缓存配额推算「磁盘满盘天数」只会产出假预警。
        if self.ctx.cluster.is_shared_data() {
            return Ok("本集群为存算分离（shared-data）：数据存放于对象存储，计算节点本地盘仅承载数据缓存，\
                       缓存写满后按 LRU 淘汰，不构成容量风险。容量规划请以对象存储用量为准。"
                .to_string());
        }
        let forecast = forecast_disk(&self.ctx.pool, self.ctx.cluster.id)
            .await
            .map_err(|e| format!("容量预测失败: {}", e))?;
        match forecast {
            None => {
                Ok("当前数据不足或磁盘趋势平稳：近 24h 增长不显著（<1.2%/天），暂无满盘风险预测。"
                    .to_string())
            },
            Some(f) => {
                if let Some(days) = f.eta_days {
                    Ok(format!(
                        "磁盘当前 {:.1}%，近 24h 增长 {:.2}%/天，预计 {:.1} 天后达到 100%（采样 {} 点）。\
                         若导入速率不变，建议在 {:.1} 天内完成清理或扩容规划。",
                        f.current_pct, f.slope_pct_per_day, days, f.points, days
                    ))
                } else if f.current_pct >= 90.0 {
                    Ok(format!(
                        "磁盘当前 {:.1}% 已接近满盘（近 24h 增长 {:.2}%/天），预测天数已无意义，\
                         建议立即清理过期数据或扩容。",
                        f.current_pct, f.slope_pct_per_day
                    ))
                } else {
                    Ok(format!(
                        "磁盘当前 {:.1}%（近 24h 增长 {:.2}%/天，采样 {} 点），当前增速未达预测门槛，\
                         暂无明确满盘时间。",
                        f.current_pct, f.slope_pct_per_day, f.points
                    ))
                }
            },
        }
    }
}
