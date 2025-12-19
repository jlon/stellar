//! Metrics parsing utilities for StarRocks profile operators
//!
//! Handles extraction and parsing of CommonMetrics and UniqueMetrics blocks.

use crate::services::profile_analyzer::models::OperatorMetrics;
use crate::services::profile_analyzer::parser::core::ValueParser;
use crate::services::profile_analyzer::parser::core::operator_parser::OperatorParser;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

// Counter regex: matches "- CounterName: value" format
// According to Doris profile-dag-parser.md: `^- ([^:]+): (.+)`
// This allows counter names with special characters (e.g., "Counter-Name", "Counter.Name")
// but still requires the colon separator
static METRIC_LINE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*-\s+([^:]+):\s*(.+)$").unwrap());

/// Parser for operator metrics
pub struct MetricsParser;

impl MetricsParser {
    /// Parse common metrics from operator text block
    pub fn parse_common_metrics(text: &str) -> OperatorMetrics {
        Self::parse_metrics_from_text(text)
    }

    /// Create OperatorMetrics from a HashMap of key-value pairs
    pub fn from_hashmap(map: &HashMap<String, String>) -> OperatorMetrics {
        let mut metrics = OperatorMetrics::default();

        for (key, value) in map {
            Self::set_metric_value(&mut metrics, key, value);
        }

        metrics
    }

    /// Merge memory-related metrics from unique_metrics into OperatorMetrics
    /// This handles metrics like HashTableMemoryUsage, LocalExchangePeakMemoryUsage, etc.
    pub fn merge_memory_metrics(
        metrics: &mut OperatorMetrics,
        unique_metrics: &HashMap<String, String>,
    ) {
        for (key, value) in unique_metrics {
            if key.contains("Memory") || key.contains("memory") {
                Self::set_metric_value(metrics, key, value);
            }
        }
    }

    /// Parse metrics from raw text
    pub fn parse_metrics_from_text(text: &str) -> OperatorMetrics {
        let mut metrics = OperatorMetrics::default();

        for line in text.lines() {
            if let Some((key, value)) = Self::parse_metric_line(line) {
                Self::set_metric_value(&mut metrics, &key, &value);
            }
        }

        metrics
    }

    /// Parse a single metric line
    ///
    /// Format: "- MetricName: value"
    pub fn parse_metric_line(line: &str) -> Option<(String, String)> {
        METRIC_LINE_REGEX.captures(line).and_then(|caps| {
            let key = caps.get(1)?.as_str().trim().to_string();
            let value = caps.get(2)?.as_str().trim().to_string();
            Some((key, value))
        })
    }

    /// Extract CommonMetrics block from operator text
    /// Supports both StarRocks format (CommonMetrics:) and Doris format (CommonCounters:)
    pub fn extract_common_metrics_block(text: &str) -> String {
        // Try Doris format first (CommonCounters:), then fallback to StarRocks format (CommonMetrics:)
        let doris_result = Self::extract_section_block(text, "CommonCounters:");
        if !doris_result.trim().is_empty() {
            return doris_result;
        }
        Self::extract_section_block(text, "CommonMetrics:")
    }

    /// Extract UniqueMetrics block from operator text
    /// Supports both StarRocks format (UniqueMetrics:) and Doris format (CustomCounters:)
    pub fn extract_unique_metrics_block(text: &str) -> String {
        // Try Doris format first (CustomCounters:), then fallback to StarRocks format (UniqueMetrics:)
        let doris_result = Self::extract_section_block(text, "CustomCounters:");
        if !doris_result.trim().is_empty() {
            return doris_result;
        }
        Self::extract_section_block(text, "UniqueMetrics:")
    }

    /// Extract a section block from text
    fn extract_section_block(text: &str, section_marker: &str) -> String {
        if let Some(start) = text.find(section_marker) {
            let after_marker = &text[start + section_marker.len()..];
            let lines: Vec<&str> = after_marker.lines().collect();

            if lines.is_empty() {
                return String::new();
            }

            let mut block_lines = Vec::new();

            let marker_line_start = text[..start].rfind('\n').map(|p| p + 1).unwrap_or(0);
            let marker_line = &text[marker_line_start..start + section_marker.len()];
            let marker_indent = Self::get_indent_level(marker_line);

            for line in lines {
                let trimmed = line.trim();

                if trimmed.is_empty() {
                    // According to Doris ProfileDagParser.java, empty lines stop counter parsing
                    // But we continue to include them as they might be part of the block
                    block_lines.push(line);
                    continue;
                }

                let current_indent = Self::get_indent_level(line);

                // Stop at next Metrics/Counters section (StarRocks: Metrics:, Doris: Counters:)
                // Only stop if it's at same or less indentation level
                if (trimmed.ends_with("Metrics:") || trimmed.ends_with("Counters:"))
                    && current_indent <= marker_indent
                {
                    break;
                }

                // Stop at operator header (StarRocks: plan_node_id=, Doris: id=)
                // According to Doris ProfileDagParser.java, operator headers end the counter section
                if (trimmed.contains("(plan_node_id=") || trimmed.contains("(id="))
                    && !trimmed.starts_with("-")
                    && OperatorParser::is_operator_header(trimmed)
                {
                    break;
                }

                // Stop at Pipeline header (StarRocks: Pipeline (id=), Doris: Pipeline X(instance_num=))
                // According to Doris ProfileDagParser.java, pipeline headers end the current operator
                if trimmed.starts_with("Pipeline (id=")
                    || (trimmed.starts_with("Pipeline") && trimmed.contains("instance_num="))
                {
                    break;
                }

                // Stop at Fragment header
                // According to Doris ProfileDagParser.java, fragment headers end the current pipeline
                if trimmed.starts_with("Fragment ") && trimmed.contains(":") {
                    break;
                }

                block_lines.push(line);
            }

            block_lines.join("\n")
        } else {
            String::new()
        }
    }

    /// Get indentation level of a line
    fn get_indent_level(line: &str) -> usize {
        line.len() - line.trim_start().len()
    }

    /// Set a metric value on OperatorMetrics based on key
    /// Supports both StarRocks metric names and Doris Counter names
    fn set_metric_value(metrics: &mut OperatorMetrics, key: &str, value: &str) {
        match key {
            // StarRocks: OperatorTotalTime, Doris: ExecTime
            "OperatorTotalTime" | "ExecTime" => {
                // Doris ExecTime format: "avg 2.927ms, max 2.927ms, min 2.927ms"
                // Extract avg value for parsing
                let time_value = if value.contains("avg") {
                    value
                        .split("avg")
                        .nth(1)
                        .and_then(|s| s.split(',').next())
                        .map(|s| s.trim())
                        .unwrap_or(value)
                } else {
                    value
                };

                metrics.operator_total_time_raw = Some(value.to_string());
                if let Ok(duration) = ValueParser::parse_duration(time_value) {
                    metrics.operator_total_time = Some(duration.as_nanos() as u64);
                }
            },
            "__MIN_OF_OperatorTotalTime" => {
                if let Ok(duration) = ValueParser::parse_duration(value) {
                    metrics.operator_total_time_min = Some(duration.as_nanos() as u64);
                }
            },
            "__MAX_OF_OperatorTotalTime" => {
                if let Ok(duration) = ValueParser::parse_duration(value) {
                    metrics.operator_total_time_max = Some(duration.as_nanos() as u64);
                }
            },
            "CPUTime" => {
                if let Ok(duration) = ValueParser::parse_duration(value) {
                    metrics.operator_total_time = Some(duration.as_nanos() as u64);
                }
            },
            "PushChunkNum" | "BlocksProduced" => {
                // Doris uses BlocksProduced instead of PushChunkNum
                // Format: "sum 1, avg 1, max 1, min 1" or single value
                let chunk_value = if value.contains("sum") {
                    value
                        .split("sum")
                        .nth(1)
                        .and_then(|s| s.split(',').next())
                        .map(|s| s.trim())
                        .unwrap_or(value)
                } else {
                    value
                };
                metrics.push_chunk_num = Self::extract_number(chunk_value);
            },
            // StarRocks: PushRowNum, Doris: InputRows (for SinkOperator)
            "PushRowNum" | "InputRows" => {
                // Doris InputRows format: "sum 100, avg 100, max 100, min 100"
                // Extract sum value for parsing
                let row_value = if value.contains("sum") {
                    value
                        .split("sum")
                        .nth(1)
                        .and_then(|s| s.split(',').next())
                        .map(|s| s.trim())
                        .unwrap_or(value)
                } else {
                    value
                };
                metrics.push_row_num = Self::extract_number(row_value);
            },
            "PullChunkNum" => {
                metrics.pull_chunk_num = Self::extract_number(value);
            },
            // StarRocks: PullRowNum, Doris: RowsProduced (for non-SinkOperator)
            "PullRowNum" | "RowsProduced" => {
                // Doris RowsProduced format: "sum 100, avg 100, max 100, min 100"
                // Extract sum value for parsing
                let row_value = if value.contains("sum") {
                    value
                        .split("sum")
                        .nth(1)
                        .and_then(|s| s.split(',').next())
                        .map(|s| s.trim())
                        .unwrap_or(value)
                } else {
                    value
                };
                metrics.pull_row_num = Self::extract_number(row_value);
            },
            "PushTotalTime" => {
                if let Ok(duration) = ValueParser::parse_duration(value) {
                    metrics.push_total_time = Some(duration.as_nanos() as u64);
                }
            },
            "__MIN_OF_PushTotalTime" => {
                if let Ok(duration) = ValueParser::parse_duration(value) {
                    metrics.push_total_time_min = Some(duration.as_nanos() as u64);
                }
            },
            "__MAX_OF_PushTotalTime" => {
                if let Ok(duration) = ValueParser::parse_duration(value) {
                    metrics.push_total_time_max = Some(duration.as_nanos() as u64);
                }
            },
            "PullTotalTime" => {
                if let Ok(duration) = ValueParser::parse_duration(value) {
                    metrics.pull_total_time = Some(duration.as_nanos() as u64);
                }
            },
            "__MIN_OF_PullTotalTime" => {
                if let Ok(duration) = ValueParser::parse_duration(value) {
                    metrics.pull_total_time_min = Some(duration.as_nanos() as u64);
                }
            },
            "__MAX_OF_PullTotalTime" => {
                if let Ok(duration) = ValueParser::parse_duration(value) {
                    metrics.pull_total_time_max = Some(duration.as_nanos() as u64);
                }
            },
            // StarRocks: MemoryUsage, Doris: MemoryUsage (same name)
            "MemoryUsage" | "MemoryUsagePeak" => {
                // Doris MemoryUsage format: "sum 0.00 , avg 0.00 , max 0.00 , min 0.00"
                let mem_value = if value.contains("sum") {
                    value
                        .split("sum")
                        .nth(1)
                        .and_then(|s| s.split(',').next())
                        .map(|s| s.trim())
                        .unwrap_or(value)
                } else {
                    value
                };
                // Try to parse bytes, if fails, try to parse as number
                if let Ok(bytes) = ValueParser::parse_bytes(mem_value) {
                    let current = metrics.memory_usage.unwrap_or(0);
                    metrics.memory_usage = Some(current.max(bytes));
                } else if let Some(num) = Self::extract_number(mem_value) {
                    let current = metrics.memory_usage.unwrap_or(0);
                    metrics.memory_usage = Some(current.max(num));
                }
            },

            "LocalExchangePeakMemoryUsage"
            | "HashTableMemoryUsage"
            | "PeakChunkBufferMemoryUsage"
            | "PassThroughBufferPeakMemoryUsage"
            | "PeakBufferMemoryBytes"
            | "OperatorPeakMemoryUsage"
            | "AggregatorMemoryUsage"
            | "SortMemoryUsage" => {
                if let Ok(bytes) = ValueParser::parse_bytes(value) {
                    let current = metrics.memory_usage.unwrap_or(0);
                    metrics.memory_usage = Some(current + bytes);
                }
            },
            "OutputChunkBytes" => {
                metrics.output_chunk_bytes = ValueParser::parse_bytes(value).ok();
            },
            _ => {},
        }
    }

    fn extract_number(value: &str) -> Option<u64> {
        ValueParser::parse_number(value).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_metric_line() {
        let line = "     - OperatorTotalTime: 123ms";
        let result = MetricsParser::parse_metric_line(line);
        assert!(result.is_some());
        let (key, value) = result.unwrap();
        assert_eq!(key, "OperatorTotalTime");
        assert_eq!(value, "123ms");
    }

    #[test]
    fn test_from_hashmap() {
        let mut map = HashMap::new();
        map.insert("OperatorTotalTime".to_string(), "100ms".to_string());
        map.insert("PushRowNum".to_string(), "1000".to_string());

        let metrics = MetricsParser::from_hashmap(&map);
        assert_eq!(metrics.operator_total_time, Some(100_000_000));
        assert_eq!(metrics.push_row_num, Some(1000));
    }
}
