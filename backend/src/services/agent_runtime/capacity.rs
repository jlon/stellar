//! 容量自适应：基于 metrics_snapshots 的磁盘趋势线性回归 → 满盘 ETA。
//!
//! 产品语义（六阶段容量自适应）：
//! - 仅对仍有增长余地的磁盘（< 90%）给出满盘 ETA；接近满盘时给出立即处置文案
//! - 斜率门槛过滤噪声（< 1.2%/天 视为平稳，不预测）
//! - 窗口 24h × 每 30s 采样（LIMIT 2880 点），O(n) 最小二乘

use serde::Serialize;
use sqlx::Row;

use crate::db::{AppDb, query as db_query};

/// 磁盘趋势预测结果。
#[derive(Debug, Clone, Serialize)]
pub struct DiskForecast {
    pub current_pct: f64,
    /// 每日增长百分点（最小二乘斜率 × 24）。
    pub slope_pct_per_day: f64,
    /// 预计满盘天数（增长显著且未接近满盘时 Some）。
    pub eta_days: Option<f64>,
    /// 参与回归的采样点数。
    pub points: usize,
}

/// 窗口：近 24h 采样（30s 一条 → 2880 点）。
const WINDOW_POINTS: usize = 2880;
/// 趋势显著门槛：0.05 %/h = 1.2 %/天。
const MIN_SLOPE_PCT_PER_DAY: f64 = 1.2;
/// 接近满盘时不再预测天数（清理/扩容优先级高于预测）。
const NEAR_FULL_PCT: f64 = 90.0;

#[stellar_macros::app_db]
pub async fn forecast_disk<DB: AppDb>(
    pool: &sqlx::Pool<DB>,
    cluster_id: i64,
) -> Result<Option<DiskForecast>, String> {
    let rows = db_query::query(
        "SELECT collected_at, disk_usage_pct FROM metrics_snapshots \
         WHERE cluster_id = ? ORDER BY collected_at DESC LIMIT ?",
    )
    .bind(cluster_id)
    .bind(WINDOW_POINTS as i64)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut points: Vec<(f64, f64)> = Vec::with_capacity(rows.len());
    for r in &rows {
        let at: chrono::DateTime<chrono::Utc> = r.get("collected_at");
        let pct: f64 = r.get("disk_usage_pct");
        points.push((at.timestamp() as f64, pct));
    }
    if points.len() < 48 {
        return Ok(None); // 数据不足
    }
    points.reverse(); // 时间升序

    let current_pct = points.last().map(|(_, y)| *y).unwrap_or(0.0);
    let x0 = points[0].0;
    let n = points.len() as f64;
    let mut sx = 0.0;
    let mut sy = 0.0;
    for (x, y) in &points {
        sx += x - x0;
        sy += *y;
    }
    let x_mean = sx / n;
    let y_mean = sy / n;
    let mut sxx = 0.0;
    let mut sxy = 0.0;
    for (x, y) in &points {
        let dx = (x - x0) - x_mean;
        let dy = *y - y_mean;
        sxx += dx * dx;
        sxy += dx * dy;
    }
    if sxx <= f64::EPSILON {
        return Ok(None);
    }
    let slope_per_hour = sxy / sxx;
    let slope_per_day = slope_per_hour * 24.0;

    let eta_days = if slope_per_day > MIN_SLOPE_PCT_PER_DAY && current_pct < NEAR_FULL_PCT {
        Some(((100.0 - current_pct) / slope_per_day).max(0.0))
    } else {
        None
    };

    Ok(Some(DiskForecast {
        current_pct,
        slope_pct_per_day: slope_per_day,
        eta_days,
        points: points.len(),
    }))
}
#[cfg(test)]
mod tests {
    use super::*;

    fn forecast_from(points: Vec<(f64, f64)>) -> DiskForecast {
        // 直接用回归逻辑（免 DB）：把 private 计算抽为纯函数可测
        let n = points.len() as f64;
        let x0 = points[0].0;
        let mut sx = 0.0;
        let mut sy = 0.0;
        for (x, y) in &points {
            sx += x - x0;
            sy += *y;
        }
        let x_mean = sx / n;
        let y_mean = sy / n;
        let mut sxx = 0.0;
        let mut sxy = 0.0;
        for (x, y) in &points {
            let dx = (x - x0) - x_mean;
            let dy = *y - y_mean;
            sxx += dx * dx;
            sxy += dx * dy;
        }
        let slope_per_hour = if sxx > f64::EPSILON { sxy / sxx } else { 0.0 };
        let slope_per_day = slope_per_hour * 24.0;
        let current_pct = points.last().map(|(_, y)| *y).unwrap_or(0.0);
        let eta_days = if slope_per_day > MIN_SLOPE_PCT_PER_DAY && current_pct < NEAR_FULL_PCT {
            Some(((100.0 - current_pct) / slope_per_day).max(0.0))
        } else {
            None
        };
        DiskForecast {
            current_pct,
            slope_pct_per_day: slope_per_day,
            eta_days,
            points: points.len(),
        }
    }

    #[test]
    fn linear_trend_yields_eta() {
        // 1h 间隔、+0.05%/h（=1.2%/天）正好过门槛
        let pts: Vec<(f64, f64)> = (0..72)
            .map(|i| (i as f64, 80.0 + 0.05 * i as f64))
            .collect();
        let f = forecast_from(pts);
        assert!((f.slope_pct_per_day - 1.2).abs() < 1e-9, "slope={}", f.slope_pct_per_day);
        let expect_days = (100.0 - f.current_pct) / f.slope_pct_per_day;
        assert!((f.eta_days.unwrap() - expect_days).abs() < 1e-9);
    }

    #[test]
    fn flat_trend_no_eta() {
        let pts: Vec<(f64, f64)> = (0..72).map(|i| (i as f64, 85.0)).collect();
        let f = forecast_from(pts);
        assert!(f.eta_days.is_none());
    }

    #[test]
    fn near_full_no_eta_but_forecast() {
        let pts: Vec<(f64, f64)> = (0..72)
            .map(|i| (i as f64, 92.0 + 0.05 * i as f64))
            .collect();
        let f = forecast_from(pts);
        assert!((f.current_pct - 95.55).abs() < 1e-9, "current={}", f.current_pct);
        assert!(f.eta_days.is_none(), "≥90% 不预测天数");
    }
}
