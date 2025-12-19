//! Section parser for StarRocks and Doris profile
//!
//! Parses Summary, Planner, and Execution sections from profile text.
//! Supports both StarRocks format (Query: -> Summary:) and Doris format (Summary:).

use crate::services::profile_analyzer::models::{
    ExecutionInfo, PlannerInfo, ProfileSummary, SessionVariableInfo,
};
use crate::services::profile_analyzer::parser::core::ValueParser;
use crate::services::profile_analyzer::parser::error::{ParseError, ParseResult};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

static SUMMARY_LINE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*-\s+([^:]+):\s*(.*)$").unwrap());

/// Parser for profile sections (Summary, Planner, Execution)
pub struct SectionParser;

impl SectionParser {
    /// Parse Summary section from profile text
    /// Supports both StarRocks format (Query: -> Summary:) and Doris format (Summary:)
    pub fn parse_summary(text: &str) -> ParseResult<ProfileSummary> {
        // Detect profile format: StarRocks starts with "Query:", Doris starts with "Summary:"
        let is_doris_format = text.trim_start().starts_with("Summary:");

        let summary_block = if is_doris_format {
            // Doris format: Summary: is at the root level
            Self::extract_block(text, "Summary:")?
        } else {
            // StarRocks format: Query: -> Summary:
            Self::extract_block(text, "Summary:")?
        };

        let mut fields = HashMap::new();
        let lines: Vec<&str> = summary_block.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];
            if let Some(cap) = SUMMARY_LINE_REGEX.captures(line) {
                let key = cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
                let mut value = cap
                    .get(2)
                    .map(|m| m.as_str().trim())
                    .unwrap_or("")
                    .to_string();

                if key == "Sql Statement" {
                    let mut sql_lines = vec![value.clone()];
                    i += 1;

                    while i < lines.len() {
                        let next_line = lines[i].trim();

                        if next_line.starts_with("- ")
                            || next_line.is_empty()
                            || next_line.contains("Fragment")
                        {
                            break;
                        }
                        sql_lines.push(next_line.to_string());
                        i += 1;
                    }
                    value = sql_lines.join("\n");
                    i -= 1;
                }

                fields.insert(key.to_string(), value);
            }
            i += 1;
        }

        let non_default_variables = fields
            .get("NonDefaultSessionVariables")
            .and_then(|json_str| {
                serde_json::from_str::<HashMap<String, SessionVariableInfo>>(json_str).ok()
            })
            .unwrap_or_default();

        // Map field names based on profile format
        // StarRocks: Query ID, Query State, StarRocks Version, Query Type
        // Doris: Profile ID, Task State, Doris Version (in Execution Summary), Task Type
        let query_id = if is_doris_format {
            fields.get("Profile ID").cloned().unwrap_or_default()
        } else {
            fields.get("Query ID").cloned().unwrap_or_default()
        };

        let query_state = if is_doris_format {
            fields.get("Task State").cloned().unwrap_or_default()
        } else {
            fields.get("Query State").cloned().unwrap_or_default()
        };

        // For Doris, Doris Version is in Execution Summary section, not Summary section
        let version = if is_doris_format {
            // Try to extract from Execution Summary section
            Self::extract_block(text, "Execution Summary:")
                .ok()
                .and_then(|exec_summary_block| {
                    let mut exec_fields = HashMap::new();
                    for line in exec_summary_block.lines() {
                        if let Some(cap) = SUMMARY_LINE_REGEX.captures(line) {
                            let key = cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
                            let value = cap.get(2).map(|m| m.as_str().trim()).unwrap_or("");
                            exec_fields.insert(key.to_string(), value.to_string());
                        }
                    }
                    exec_fields.get("Doris Version").cloned()
                })
                .unwrap_or_default()
        } else {
            fields.get("StarRocks Version").cloned().unwrap_or_default()
        };

        let query_type = if is_doris_format {
            fields.get("Task Type").cloned()
        } else {
            fields.get("Query Type").cloned()
        };

        Ok(ProfileSummary {
            query_id,
            start_time: fields.get("Start Time").cloned().unwrap_or_default(),
            end_time: fields.get("End Time").cloned().unwrap_or_default(),
            total_time: fields.get("Total").cloned().unwrap_or_default(),
            query_state,
            starrocks_version: version,
            sql_statement: fields.get("Sql Statement").cloned().unwrap_or_default(),
            query_type,
            user: fields.get("User").cloned(),
            default_db: fields.get("Default Db").cloned(),
            variables: HashMap::new(),
            non_default_variables,
            query_allocated_memory: None,
            query_peak_memory: None,
            total_time_ms: Self::parse_total_time_ms(
                &fields.get("Total").cloned().unwrap_or_default(),
            ),
            query_cumulative_operator_time: fields.get("QueryCumulativeOperatorTime").cloned(),
            query_cumulative_operator_time_ms: fields
                .get("QueryCumulativeOperatorTime")
                .and_then(|time_str| Self::parse_total_time_ms(time_str)),
            query_execution_wall_time: fields.get("QueryExecutionWallTime").cloned(),
            query_execution_wall_time_ms: fields
                .get("QueryExecutionWallTime")
                .and_then(|time_str| Self::parse_total_time_ms(time_str)),

            query_cumulative_cpu_time: None,
            query_cumulative_cpu_time_ms: None,
            query_cumulative_scan_time: None,
            query_cumulative_scan_time_ms: None,
            query_cumulative_network_time: None,
            query_cumulative_network_time_ms: None,
            query_peak_schedule_time: None,
            query_peak_schedule_time_ms: None,
            result_deliver_time: None,
            result_deliver_time_ms: None,

            planner_total_time: None,
            planner_total_time_ms: None,
            collect_profile_time: None,
            collect_profile_time_ms: None,

            io_seek_time: None,
            io_seek_time_ms: None,
            local_disk_read_io_time: None,
            local_disk_read_io_time_ms: None,
            remote_read_io_time: None,
            remote_read_io_time_ms: None,

            total_raw_rows_read: None,
            total_bytes_read: None,
            total_bytes_read_display: None,
            pages_count_memory: None,
            pages_count_local_disk: None,
            pages_count_remote: None,
            result_rows: None,
            result_bytes: None,
            result_bytes_display: None,

            query_sum_memory_usage: None,
            query_deallocated_memory_usage: None,

            query_spill_bytes: None,

            datacache_hit_rate: None,
            datacache_bytes_local: None,
            datacache_bytes_remote: None,
            datacache_bytes_local_display: None,
            datacache_bytes_remote_display: None,

            top_time_consuming_nodes: None,

            is_profile_async: fields
                .get("IsProfileAsync")
                .map(|v| v.eq_ignore_ascii_case("true")),
            retry_times: fields.get("Retry Times").and_then(|v| v.parse().ok()),

            missing_instance_count: None,
            total_instance_count: None,
            is_profile_complete: None,
            profile_completeness_warning: None,
        })
    }

    /// Parse Planner section from profile text
    /// For Doris format, this will return empty PlannerInfo since Doris doesn't have Planner section
    pub fn parse_planner(text: &str) -> ParseResult<PlannerInfo> {
        use crate::services::profile_analyzer::models::HMSMetrics;

        // Doris format doesn't have Planner section
        if text.trim_start().starts_with("Summary:") {
            return Ok(PlannerInfo::default());
        }

        let planner_block = Self::extract_block(text, "Planner:")?;
        let mut details = HashMap::new();
        let mut hms_metrics = HMSMetrics::default();
        let mut total_time_ms = 0.0;
        let mut optimizer_time_ms = 0.0;

        for line in planner_block.lines() {
            let trimmed = line.trim().trim_start_matches('-').trim();

            if trimmed.contains("HMS.") {
                if let Some((name, time)) = Self::parse_hms_metric(trimmed) {
                    match name.as_str() {
                        "getDatabase" => hms_metrics.get_database_ms += time,
                        "getTable" => hms_metrics.get_table_ms += time,
                        "getPartitionsByNames" => hms_metrics.get_partitions_ms += time,
                        "getPartitionColumnStats" => hms_metrics.get_partition_stats_ms += time,
                        "listPartitionNamesByValue" | "listPartitionNames" => {
                            hms_metrics.list_partition_names_ms += time
                        },
                        "PARTITIONS.LIST_FS_PARTITIONS" | "PARTITIONS.LIST_FS_ASYNC.WAIT" => {
                            hms_metrics.list_fs_partitions_ms += time
                        },
                        _ => {},
                    }
                }
            } else if trimmed.starts_with("Total[") {
                if let Some(time) = Self::parse_planner_time(trimmed) {
                    total_time_ms = time;
                }
            } else if trimmed.starts_with("Optimizer[") {
                if let Some(time) = Self::parse_planner_time(trimmed) {
                    optimizer_time_ms = time;
                }
            } else if let Some(cap) = SUMMARY_LINE_REGEX.captures(line) {
                let key = cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
                let value = cap.get(2).map(|m| m.as_str().trim()).unwrap_or("");
                details.insert(key.to_string(), value.to_string());
            }
        }

        hms_metrics.total_hms_time_ms = hms_metrics.get_database_ms
            + hms_metrics.get_table_ms
            + hms_metrics.get_partitions_ms
            + hms_metrics.get_partition_stats_ms
            + hms_metrics.list_partition_names_ms
            + hms_metrics.list_fs_partitions_ms;

        Ok(PlannerInfo { details, hms_metrics, total_time_ms, optimizer_time_ms })
    }

    /// Parse HMS metric line: "HMS.getTable[2] 29ms" or "HMS.PARTITIONS.LIST_FS_PARTITIONS[4] 350ms"
    fn parse_hms_metric(line: &str) -> Option<(String, f64)> {
        use super::ValueParser;

        let hms_start = line.find("HMS.")?;
        let rest = &line[hms_start + 4..];

        let bracket_pos = rest.find('[')?;
        let name = rest[..bracket_pos].to_string();

        let close_bracket = rest.find(']')?;
        let time_str = rest[close_bracket + 1..].trim();

        let duration = ValueParser::parse_duration(time_str).ok()?;
        let time_ms = duration.as_millis() as f64;

        Some((name, time_ms))
    }

    /// Parse planner time line: "Total[1] 1s570ms" -> 1570.0
    fn parse_planner_time(line: &str) -> Option<f64> {
        use super::ValueParser;

        let close_bracket = line.find(']')?;
        let time_str = line[close_bracket + 1..].trim();
        let duration = ValueParser::parse_duration(time_str).ok()?;
        Some(duration.as_millis() as f64)
    }

    /// Parse Execution section from profile text
    /// For Doris format, this will return empty ExecutionInfo since Doris uses MergedProfile instead
    pub fn parse_execution(text: &str) -> ParseResult<ExecutionInfo> {
        // Doris format doesn't have Execution section, return empty ExecutionInfo
        if text.trim_start().starts_with("Summary:") {
            return Ok(ExecutionInfo { topology: String::new(), metrics: HashMap::new() });
        }

        let execution_block = Self::extract_block(text, "Execution:")?;

        let topology = Self::extract_topology(&execution_block)?;

        let mut metrics = HashMap::new();
        for line in execution_block.lines() {
            if let Some(cap) = SUMMARY_LINE_REGEX.captures(line) {
                let key = cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
                let value = cap.get(2).map(|m| m.as_str().trim()).unwrap_or("");
                if !key.is_empty() && !value.is_empty() && key != "Topology" {
                    metrics.insert(key.to_string(), value.to_string());
                }
            }
        }

        Ok(ExecutionInfo { topology, metrics })
    }

    /// Extract a block of text for a given section marker
    fn extract_block(text: &str, section_marker: &str) -> ParseResult<String> {
        if let Some(start) = text.find(section_marker) {
            let before_marker = &text[..start];
            let marker_line_start = before_marker.rfind('\n').map(|pos| pos + 1).unwrap_or(0);
            let marker_line = &text[marker_line_start..start + section_marker.len()];
            let marker_indent = Self::get_indent(marker_line);

            let rest = &text[start + section_marker.len()..];
            let lines: Vec<&str> = rest.lines().collect();

            let mut end_pos = rest.len();
            for (i, line) in lines.iter().enumerate().skip(1) {
                if !line.trim().is_empty() {
                    let curr_indent = Self::get_indent(line);

                    if curr_indent <= marker_indent && line.trim().ends_with(':') {
                        let mut char_count = 0;
                        for l in lines.iter().take(i) {
                            char_count += l.len() + 1;
                        }
                        end_pos = char_count;
                        break;
                    }
                }
            }

            Ok(rest[..end_pos].to_string())
        } else {
            Err(ParseError::SectionNotFound(section_marker.to_string()))
        }
    }

    /// Extract topology JSON from execution block
    fn extract_topology(text: &str) -> ParseResult<String> {
        if let Some(start_pos) = text.find("- Topology:") {
            let after_label = &text[start_pos + 11..];
            if let Some(json_start) = after_label.find('{') {
                let json_part = &after_label[json_start..];

                let mut depth = 0;
                let mut end_pos = 0;

                for (i, ch) in json_part.char_indices() {
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end_pos = i + 1;
                                break;
                            }
                        },
                        _ => {},
                    }
                }

                if end_pos > 0 {
                    return Ok(json_part[..end_pos].to_string());
                }
            }
        }

        Ok(String::new())
    }

    /// Get indentation level of a line
    fn get_indent(line: &str) -> usize {
        line.chars().take_while(|c| c.is_whitespace()).count()
    }

    /// Parse total time string to milliseconds
    fn parse_total_time_ms(time_str: &str) -> Option<f64> {
        ValueParser::parse_time_to_ms(time_str).ok()
    }

    /// Extract execution metrics from Doris Execution Summary section
    /// Maps Doris Execution Summary fields to StarRocks ProfileSummary fields
    pub fn extract_doris_execution_summary_metrics(text: &str, summary: &mut ProfileSummary) {
        let exec_summary_block = match Self::extract_block(text, "Execution Summary:") {
            Ok(block) => block,
            Err(_) => return, // Execution Summary not found, skip
        };

        let mut fields = HashMap::new();
        for line in exec_summary_block.lines() {
            if let Some(cap) = SUMMARY_LINE_REGEX.captures(line) {
                let key = cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
                let value = cap.get(2).map(|m| m.as_str().trim()).unwrap_or("");
                fields.insert(key.to_string(), value.to_string());
            }
        }

        // Map Doris Execution Summary fields to StarRocks ProfileSummary fields
        // Execution time: Use Total Time from Summary section (already parsed) as the wall time
        // This represents the total query execution time from start to finish
        if summary.query_execution_wall_time.is_none() && !summary.total_time.is_empty() {
            summary.query_execution_wall_time = Some(summary.total_time.clone());
            // total_time_ms is already parsed in parse_summary
            summary.query_execution_wall_time_ms = summary.total_time_ms;
        }

        // Processing time: For Doris, Plan Time + Schedule Time + Wait and Fetch Result Time
        // represents the total query execution time, not the cumulative operator time.
        // However, we still set it here for overview display purposes.
        // The actual cumulative operator time (sum of all operator ExecTime) will be calculated
        // from fragments in TreeBuilder::determine_base_time when calculating time_percentage.
        if summary.query_cumulative_operator_time.is_none() {
            let plan_time = fields
                .get("Plan Time")
                .and_then(|t| ValueParser::parse_time_to_ms(t).ok())
                .unwrap_or(0.0);
            let schedule_time = fields
                .get("Schedule Time")
                .and_then(|t| ValueParser::parse_time_to_ms(t).ok())
                .unwrap_or(0.0);
            let fetch_time = fields
                .get("Wait and Fetch Result Time")
                .and_then(|t| ValueParser::parse_time_to_ms(t).ok())
                .unwrap_or(0.0);
            let total_processing_ms = plan_time + schedule_time + fetch_time;
            if total_processing_ms > 0.0 {
                summary.query_cumulative_operator_time_ms = Some(total_processing_ms);
                summary.query_cumulative_operator_time =
                    Some(format!("{}ms", total_processing_ms as u64));
            }
        }

        // Planner time: Plan Time
        if summary.planner_total_time.is_none() {
            if let Some(plan_time) = fields.get("Plan Time") {
                summary.planner_total_time = Some(plan_time.clone());
                summary.planner_total_time_ms = ValueParser::parse_time_to_ms(plan_time).ok();
            }
        }

        // Schedule time: Schedule Time
        if summary.query_peak_schedule_time.is_none() {
            if let Some(schedule_time) = fields.get("Schedule Time") {
                summary.query_peak_schedule_time = Some(schedule_time.clone());
                summary.query_peak_schedule_time_ms =
                    ValueParser::parse_time_to_ms(schedule_time).ok();
            }
        }

        // Result deliver time: Write Result Time
        if summary.result_deliver_time.is_none() {
            if let Some(write_time) = fields.get("Write Result Time") {
                summary.result_deliver_time = Some(write_time.clone());
                summary.result_deliver_time_ms = ValueParser::parse_time_to_ms(write_time).ok();
            }
        }

        // Total instances count
        if summary.total_instance_count.is_none() {
            if let Some(instances) = fields.get("Total Instances Num") {
                summary.total_instance_count = instances.parse().ok();
            }
        }

        // Parallel fragment exec instance num
        if summary.total_instance_count.is_none() {
            if let Some(instances) = fields.get("Parallel Fragment Exec Instance Num") {
                summary.total_instance_count = instances.parse().ok();
            }
        }
    }

    /// Extract execution metrics and update summary
    pub fn extract_execution_metrics(execution_info: &ExecutionInfo, summary: &mut ProfileSummary) {
        if let Some(val) = execution_info.metrics.get("QueryAllocatedMemoryUsage") {
            summary.query_allocated_memory = ValueParser::parse_bytes(val).ok();
        }
        if let Some(val) = execution_info.metrics.get("QueryPeakMemoryUsagePerNode") {
            summary.query_peak_memory = ValueParser::parse_bytes(val).ok();
        }
        if let Some(val) = execution_info.metrics.get("QuerySumMemoryUsage") {
            summary.query_sum_memory_usage = Some(val.clone());
        }
        if let Some(val) = execution_info.metrics.get("QueryDeallocatedMemoryUsage") {
            summary.query_deallocated_memory_usage = Some(val.clone());
        }

        if let Some(val) = execution_info.metrics.get("QueryCumulativeCpuTime") {
            summary.query_cumulative_cpu_time = Some(val.clone());
            summary.query_cumulative_cpu_time_ms = ValueParser::parse_time_to_ms(val).ok();
        }
        if let Some(val) = execution_info.metrics.get("QueryCumulativeScanTime") {
            summary.query_cumulative_scan_time = Some(val.clone());
            summary.query_cumulative_scan_time_ms = ValueParser::parse_time_to_ms(val).ok();
        }
        if let Some(val) = execution_info.metrics.get("QueryCumulativeNetworkTime") {
            summary.query_cumulative_network_time = Some(val.clone());
            summary.query_cumulative_network_time_ms = ValueParser::parse_time_to_ms(val).ok();
        }
        if let Some(val) = execution_info.metrics.get("QueryPeakScheduleTime") {
            summary.query_peak_schedule_time = Some(val.clone());
            summary.query_peak_schedule_time_ms = ValueParser::parse_time_to_ms(val).ok();
        }
        if let Some(val) = execution_info.metrics.get("ResultDeliverTime") {
            summary.result_deliver_time = Some(val.clone());
            summary.result_deliver_time_ms = ValueParser::parse_time_to_ms(val).ok();
        }

        if let Some(val) = execution_info.metrics.get("QuerySpillBytes") {
            summary.query_spill_bytes = Some(val.clone());
        }

        if let Some(val) = execution_info.metrics.get("PlannerTotalTime") {
            summary.planner_total_time = Some(val.clone());
            summary.planner_total_time_ms = ValueParser::parse_time_to_ms(val).ok();
        }
        if let Some(val) = execution_info.metrics.get("CollectProfileTime") {
            summary.collect_profile_time = Some(val.clone());
            summary.collect_profile_time_ms = ValueParser::parse_time_to_ms(val).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_summary() {
        let profile = r#"
Query:
  Summary:
     - Query ID: b1f9a935-a967-11f0-b3d8-f69e292b7593
     - Start Time: 2025-10-15 09:38:48
     - Total: 1h30m
     - Query State: Finished
"#;
        let summary = SectionParser::parse_summary(profile).unwrap();
        assert_eq!(summary.query_id, "b1f9a935-a967-11f0-b3d8-f69e292b7593");
        assert_eq!(summary.total_time, "1h30m");
    }
}
