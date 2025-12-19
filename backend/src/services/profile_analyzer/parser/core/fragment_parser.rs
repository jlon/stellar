//! Fragment parser for StarRocks profile
//!
//! Parses Fragment and Pipeline structures from profile text.

use crate::services::profile_analyzer::models::{Fragment, Operator, Pipeline};
use crate::services::profile_analyzer::parser::core::{MetricsParser, OperatorParser};
use crate::services::profile_analyzer::parser::error::ParseResult;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

static FRAGMENT_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*Fragment\s+(\d+):").unwrap());

// Support both StarRocks format: Pipeline (id=0): and Doris format: Pipeline : 0(instance_num=1):
// StarRocks: "Pipeline (id=0):" -> capture group 1
// Doris: "Pipeline : 0(instance_num=1):" or "Pipeline 0(instance_num=1):" -> capture group 2 (id) and group 3 (instance_num)
// According to Doris profile-dag-parser.md: `^Pipeline (\\d+)\\(instance_num=(\\d+)\\):`
// Note: Doris can have space and colon: "Pipeline : 0" or just "Pipeline 0"
static PIPELINE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*Pipeline\s*:?\s*(?:\(id=(\d+)\)|(\d+)\(instance_num=(\d+)\))").unwrap());

/// Parser for Fragment and Pipeline structures
pub struct FragmentParser;

impl FragmentParser {
    /// Parse a single fragment from text
    pub fn parse_fragment(text: &str, id: &str) -> ParseResult<Fragment> {
        let backend_addresses = Self::extract_backend_addresses(text);
        let instance_ids = Self::extract_instance_ids(text);
        let pipelines = Self::parse_pipelines(text)?;

        Ok(Fragment { id: id.to_string(), backend_addresses, instance_ids, pipelines })
    }

    /// Extract all fragments from profile text
    pub fn extract_all_fragments(text: &str) -> Vec<Fragment> {
        let mut fragments = Vec::new();
        let lines: Vec<&str> = text.lines().collect();

        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];

            if let Some(caps) = FRAGMENT_REGEX.captures(line.trim()) {
                let id = caps.get(1).unwrap().as_str().to_string();
                let start_idx = i;
                let base_indent = Self::get_indent(line);

                let mut end_idx = lines.len();
                for (j, line) in lines.iter().enumerate().skip(i + 1) {
                    let next_indent = Self::get_indent(line);
                    if next_indent <= base_indent && FRAGMENT_REGEX.is_match(line.trim()) {
                        end_idx = j;
                        break;
                    }
                }

                let fragment_text = lines[start_idx..end_idx].join("\n");

                if let Ok(fragment) = Self::parse_fragment(&fragment_text, &id) {
                    fragments.push(fragment);
                }

                i = end_idx;
            } else {
                i += 1;
            }
        }

        fragments
    }

    /// Parse pipelines from fragment text
    fn parse_pipelines(text: &str) -> ParseResult<Vec<Pipeline>> {
        let mut pipelines = Vec::new();
        let lines: Vec<&str> = text.lines().collect();

        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];

            if let Some(caps) = PIPELINE_REGEX.captures(line.trim()) {
                // Support both formats: StarRocks (id=0) -> group 1, Doris (0(instance_num=) -> group 2
                let id = caps
                    .get(1)
                    .or_else(|| caps.get(2))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_else(|| "0".to_string());
                let start_idx = i;
                let base_indent = Self::get_indent(line);

                let mut end_idx = lines.len();
                for (j, line) in lines.iter().enumerate().skip(i + 1) {
                    let next_indent = Self::get_indent(line);
                    if next_indent <= base_indent
                        && (PIPELINE_REGEX.is_match(line.trim())
                            || FRAGMENT_REGEX.is_match(line.trim()))
                    {
                        end_idx = j;
                        break;
                    }
                }

                let pipeline_text = lines[start_idx..end_idx].join("\n");
                let pipeline = Self::parse_single_pipeline(&pipeline_text, &id)?;
                pipelines.push(pipeline);
                i = end_idx;
            } else {
                i += 1;
            }
        }

        Ok(pipelines)
    }

    /// Parse a single pipeline
    fn parse_single_pipeline(text: &str, id: &str) -> ParseResult<Pipeline> {
        let metrics = Self::extract_pipeline_metrics(text);
        let operators = Self::extract_operators(text);

        Ok(Pipeline { id: id.to_string(), metrics, operators })
    }

    /// Extract pipeline-level metrics
    fn extract_pipeline_metrics(text: &str) -> HashMap<String, String> {
        let mut metrics = HashMap::new();

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("- ") && trimmed.contains(": ") {
                let rest = trimmed.trim_start_matches("- ");
                let parts: Vec<&str> = rest.splitn(2, ": ").collect();
                if parts.len() == 2 {
                    metrics.insert(parts[0].trim().to_string(), parts[1].trim().to_string());
                }
            }
        }

        metrics
    }

    /// Extract operators from pipeline text
    fn extract_operators(text: &str) -> Vec<Operator> {
        let mut operators = Vec::new();
        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let trimmed = lines[i].trim();

            if OperatorParser::is_operator_header(trimmed) {
                let full_header = trimmed.trim_end_matches(':').to_string();

                // Extract operator name - support both formats:
                // StarRocks: OPERATOR_NAME (plan_node_id=0)
                // Doris: OPERATOR_NAME(id=0) or OPERATOR_NAME (id=0. nereids_id=32...) or OPERATOR_NAME (id=0. nereids_id=74. table name = xxx)
                let operator_name = if let Some(pos) = full_header.find(" (plan_node_id=") {
                    // StarRocks format: OPERATOR_NAME (plan_node_id=0)
                    full_header[..pos].trim().to_string()
                } else if let Some(pos) = full_header.find("(id=") {
                    // Doris format: OPERATOR_NAME(id=0) or OPERATOR_NAME (id=0. nereids_id=32...)
                    // Handle both cases: with space before (id=) and without space
                    let before_id = if let Some(space_pos) = full_header[..pos].rfind(' ') {
                        &full_header[..space_pos]
                    } else {
                        &full_header[..pos]
                    };
                    before_id.trim().to_string()
                } else {
                    // No plan_node_id or id, use full header
                    full_header.trim().to_string()
                };

                let base_indent = Self::get_indent(lines[i]);

                let mut operator_lines = vec![lines[i]];
                i += 1;

                while i < lines.len() {
                    let line = lines[i];
                    let trimmed = line.trim();
                    
                    if trimmed.is_empty() {
                        i += 1;
                        continue;
                    }

                    // Check if this is another operator header (even at same or smaller indent)
                    // This handles nested operators in Doris profiles
                    // For example: RESULT_SINK_OPERATOR (id=0): followed by EXCHANGE_OPERATOR (id=2):
                    if OperatorParser::is_operator_header(trimmed) {
                        break;
                    }

                    let current_indent = Self::get_indent(line);
                    // Stop if we encounter a line at same or smaller indent that's not part of this operator
                    // But allow metrics lines (starting with "-") at any indent level
                    if current_indent <= base_indent && !trimmed.starts_with("-") {
                        // Check if it's a Pipeline or Fragment header (end of current pipeline)
                        if trimmed.starts_with("Pipeline") || trimmed.starts_with("Fragment") {
                            break;
                        }
                    }

                    operator_lines.push(line);
                    i += 1;
                }

                let operator_text = operator_lines.join("\n");

                // Extract plan_node_id - support both formats
                // According to Doris ProfileDagParser.java:
                // - StarRocks: (plan_node_id=0)
                // - Doris: (id=0) or (id=0. nereids_id=32...) or (id=0. nereids_id=74. table name = xxx)
                // - Doris: (dest_id=1) for DATA_STREAM_SINK_OPERATOR when id= is not present
                // Note: Doris operator pattern: ^([A-Z_]+_OPERATOR)(?:\\([^)]+\\))?\\(id=(\\d+)\\)
                // This means id= must be followed by digits, but there can be additional info after
                let plan_node_id = if full_header.contains("plan_node_id=") {
                    // StarRocks format: (plan_node_id=0)
                    full_header
                        .split("plan_node_id=")
                        .nth(1)
                        .and_then(|s| s.split(',').next())
                        .and_then(|s| s.trim_end_matches(')').parse::<i32>().ok())
                        .map(|n| n.to_string())
                } else if let Some(id_start) = full_header.find("(id=") {
                    // Doris format: (id=0) or (id=0. nereids_id=32...) or (id=0. nereids_id=74. table name = xxx)
                    // Extract the number immediately after "id=", before any dot or closing paren
                    let after_id = &full_header[id_start + 4..]; // Skip "(id="
                    let id_str = after_id
                        .split(|c: char| c == '.' || c == ',' || c == ')')
                        .next()
                        .and_then(|s| s.trim().parse::<i32>().ok())
                        .map(|n| n.to_string());
                    id_str
                } else if let Some(dest_id_start) = full_header.find("(dest_id=") {
                    // Doris format: DATA_STREAM_SINK_OPERATOR(dest_id=1): - use dest_id as plan_node_id
                    let after_dest_id = &full_header[dest_id_start + 9..]; // Skip "(dest_id="
                    let id_str = after_dest_id
                        .split(|c: char| c == ',' || c == ')')
                        .next()
                        .and_then(|s| s.trim().parse::<i32>().ok())
                        .map(|n| n.to_string());
                    id_str
                } else {
                    None
                };

                // Extract metrics blocks - try CommonCounters/CustomCounters first (Doris format)
                // If not found, parse metrics directly from operator block (Doris MergedProfile format)
                let common_metrics_text =
                    MetricsParser::extract_common_metrics_block(&operator_text);
                let unique_metrics_text =
                    MetricsParser::extract_unique_metrics_block(&operator_text);

                // If CommonCounters section is missing, parse metrics directly from operator block
                // This handles Doris MergedProfile format where metrics appear directly under operator
                // without CommonCounters/CustomCounters sections
                let common_metrics = if common_metrics_text.trim().is_empty() {
                    // Parse metrics directly from operator block, excluding PlanInfo and nested operators
                    Self::parse_metrics_directly_from_operator_block(&operator_text)
                } else {
                    Self::parse_metrics_to_hashmap(&common_metrics_text)
                };
                
                let unique_metrics = if unique_metrics_text.trim().is_empty() {
                    // If CustomCounters is also missing, we might have some metrics in the operator block
                    // that aren't in CommonCounters. For now, return empty as CustomCounters are operator-specific.
                    HashMap::new()
                } else {
                    Self::parse_metrics_to_hashmap(&unique_metrics_text)
                };

                operators.push(Operator {
                    name: operator_name,
                    plan_node_id,
                    operator_id: None,
                    common_metrics,
                    unique_metrics,
                    children: Vec::new(),
                });
            } else {
                i += 1;
            }
        }

        operators
    }

    /// Parse metrics text to HashMap
    /// This function recursively parses nested metrics blocks like DataCache:, ORC:
    /// It flattens all nested metrics into a single level HashMap
    fn parse_metrics_to_hashmap(text: &str) -> HashMap<String, String> {
        let mut metrics = HashMap::new();

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("- ") {
                let rest = trimmed.trim_start_matches("- ");

                if let Some(colon_pos) = rest.find(": ") {
                    let key = rest[..colon_pos].trim().to_string();
                    let value = rest[colon_pos + 2..].trim().to_string();

                    if value.is_empty() {
                        continue;
                    }

                    if !key.starts_with("__MIN_OF_") {
                        metrics.insert(key, value);
                    }
                } else if !rest.is_empty() {
                }
            }
        }

        metrics
    }

    /// Parse metrics directly from operator block when CommonCounters section is missing
    /// This handles Doris MergedProfile format where metrics appear directly under operator
    /// Excludes PlanInfo section and nested operators
    fn parse_metrics_directly_from_operator_block(text: &str) -> HashMap<String, String> {
        use crate::services::profile_analyzer::parser::core::operator_parser::OperatorParser;
        
        let mut metrics = HashMap::new();
        let lines: Vec<&str> = text.lines().collect();
        
        // Skip the operator header line
        let mut i = if lines.is_empty() { 0 } else { 1 };
        let mut in_plan_info = false;
        
        while i < lines.len() {
            let line = lines[i];
            let trimmed = line.trim();
            
            // Skip empty lines
            if trimmed.is_empty() {
                i += 1;
                continue;
            }
            
            // Check if this is PlanInfo section (starts with "- PlanInfo")
            if trimmed == "- PlanInfo" {
                in_plan_info = true;
                i += 1;
                continue;
            }
            
            // Skip PlanInfo content (lines with more indent after "- PlanInfo")
            if in_plan_info {
                let current_indent = Self::get_indent(line);
                let plan_info_indent = if i > 0 {
                    Self::get_indent(lines[i - 1])
                } else {
                    0
                };
                
                // If indent decreases or we hit another metric/operator, exit PlanInfo
                if current_indent <= plan_info_indent && trimmed.starts_with("- ") {
                    in_plan_info = false;
                } else if current_indent <= plan_info_indent {
                    in_plan_info = false;
                } else {
                    i += 1;
                    continue;
                }
            }
            
            // Check if this is another operator header - stop parsing
            if OperatorParser::is_operator_header(trimmed) {
                break;
            }
            
            // Check if this is a metric line (starts with "- ")
            if trimmed.starts_with("- ") {
                let rest = trimmed.trim_start_matches("- ");
                if let Some(colon_pos) = rest.find(": ") {
                    let key = rest[..colon_pos].trim().to_string();
                    let value = rest[colon_pos + 2..].trim().to_string();
                    
                    if !value.is_empty() && !key.starts_with("__MIN_OF_") {
                        metrics.insert(key, value);
                    }
                }
            }
            
            i += 1;
        }
        
        metrics
    }

    /// Extract backend addresses from fragment text
    fn extract_backend_addresses(text: &str) -> Vec<String> {
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("- BackendAddresses:") {
                let addresses = trimmed.trim_start_matches("- BackendAddresses:").trim();
                return addresses.split(',').map(|s| s.trim().to_string()).collect();
            }
        }
        Vec::new()
    }

    /// Extract instance IDs from fragment text
    fn extract_instance_ids(text: &str) -> Vec<String> {
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("- InstanceIds:") {
                let ids = trimmed.trim_start_matches("- InstanceIds:").trim();
                return ids.split(',').map(|s| s.trim().to_string()).collect();
            }
        }
        Vec::new()
    }

    /// Get indentation level of a line
    fn get_indent(line: &str) -> usize {
        line.chars().take_while(|c| c.is_whitespace()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_backend_addresses() {
        let text = "   - BackendAddresses: 192.168.1.1:9060, 192.168.1.2:9060";
        let addrs = FragmentParser::extract_backend_addresses(text);
        assert_eq!(addrs.len(), 2);
        assert_eq!(addrs[0], "192.168.1.1:9060");
    }
}
