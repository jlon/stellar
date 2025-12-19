//! Operator parser for StarRocks and Doris profile
//!
//! Handles operator identification and node type classification.
//! Supports both StarRocks format: OPERATOR_NAME (plan_node_id=0):
//! and Doris format: OPERATOR_NAME(id=0): or OPERATOR_NAME (id=0. nereids_id=32...):

use crate::services::profile_analyzer::models::NodeType;
use once_cell::sync::Lazy;
use regex::Regex;

// Support both formats:
// StarRocks: OPERATOR_NAME (plan_node_id=0):
// Doris: OPERATOR_NAME(id=0): or OPERATOR_NAME (id=0. nereids_id=32...):
// Note: Doris can also have additional info like "OPERATOR_NAME (id=0. nereids_id=74. table name = xxx):"
static OPERATOR_HEADER_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Z_]+(?:\s*\((?:(?:plan_node_id|id)=\d+[^)]*)\))?:$").unwrap());

/// Parser for operator-related operations
pub struct OperatorParser;

impl OperatorParser {
    /// Check if a line is an operator header
    /// Supports both StarRocks format: OPERATOR_NAME (plan_node_id=0):
    /// and Doris format: OPERATOR_NAME(id=0): or OPERATOR_NAME (id=0. nereids_id=32...):
    pub fn is_operator_header(line: &str) -> bool {
        let trimmed = line.trim();

        if !trimmed.ends_with(':') {
            return false;
        }

        // Exclude Pipeline headers - they are not operators
        if trimmed.starts_with("Pipeline") {
            return false;
        }

        // StarRocks format: contains (plan_node_id=)
        if trimmed.contains("(plan_node_id=") {
            return true;
        }

        // Doris format: contains (id=) - can be (id=0) or (id=0. nereids_id=32...)
        if trimmed.contains("(id=") {
            return true;
        }

        // Doris format: some operators use dest_id= instead of id= (e.g., DATA_STREAM_SINK_OPERATOR(dest_id=1):)
        // These should also be recognized as operator headers
        if trimmed.contains("(dest_id=") && trimmed.contains("_OPERATOR") {
            return true;
        }

        OPERATOR_HEADER_REGEX.is_match(trimmed)
    }

    /// Determine node type from operator name
    pub fn determine_node_type(operator_name: &str) -> NodeType {
        let name = operator_name.to_uppercase();

        match name.as_str() {
            "OLAP_SCAN" => NodeType::OlapScan,
            "CONNECTOR_SCAN" => NodeType::ConnectorScan,
            "HASH_JOIN" | "NEST_LOOP_JOIN" | "NESTLOOP_JOIN" => NodeType::HashJoin,
            "AGGREGATE" | "AGGREGATION" => NodeType::Aggregate,
            "LIMIT" | "TOP_N" => NodeType::Limit,
            "EXCHANGE_SINK" | "LOCAL_EXCHANGE_SINK" => NodeType::ExchangeSink,
            "EXCHANGE" | "EXCHANGE_SOURCE" | "MERGE_EXCHANGE" => NodeType::ExchangeSource,
            "RESULT_SINK" => NodeType::ResultSink,
            "CHUNK_ACCUMULATE" => NodeType::ChunkAccumulate,
            "SORT" => NodeType::Sort,
            "PROJECT" => NodeType::Project,
            "TABLE_FUNCTION" => NodeType::TableFunction,
            "OLAP_TABLE_SINK" => NodeType::OlapTableSink,
            _ => NodeType::Unknown,
        }
    }

    /// Get canonical topology name for an operator
    ///
    /// Maps various operator names to their canonical form used in topology
    pub fn canonical_topology_name(operator_name: &str) -> String {
        let name = operator_name.to_uppercase();

        match name.as_str() {
            "OLAP_SCAN" | "OLAP_SCAN_OPERATOR" => "OLAP_SCAN".to_string(),
            "CONNECTOR_SCAN" | "CONNECTOR_SCAN_OPERATOR" => "CONNECTOR_SCAN".to_string(),

            "HASH_JOIN" | "HASH_JOIN_BUILD" | "HASH_JOIN_PROBE" => "HASH_JOIN".to_string(),
            "NEST_LOOP_JOIN" | "NESTLOOP_JOIN" => "NESTLOOP_JOIN".to_string(),

            "AGGREGATE" | "AGGREGATION" | "AGGREGATE_BLOCKING" | "AGGREGATE_STREAMING" => {
                "AGGREGATE".to_string()
            },

            "EXCHANGE" | "EXCHANGE_SOURCE" | "EXCHANGE_SINK" | "MERGE_EXCHANGE" => {
                "EXCHANGE".to_string()
            },

            _ => name,
        }
    }

    /// Extract operator block from profile text
    /// Supports both StarRocks format: OPERATOR_NAME (plan_node_id=0):
    /// and Doris format: OPERATOR_NAME(id=0): or OPERATOR_NAME (id=0. nereids_id=32...):
    pub fn extract_operator_block(
        text: &str,
        operator_name: &str,
        plan_node_id: Option<i32>,
    ) -> String {
        let lines: Vec<&str> = text.lines().collect();
        let mut result = Vec::new();
        let mut in_operator = false;
        let mut base_indent = 0;

        for line in lines {
            let trimmed = line.trim();

            if !in_operator {
                let is_match = if let Some(plan_id) = plan_node_id {
                    // Support both formats:
                    // StarRocks: OPERATOR_NAME (plan_node_id={plan_id}):
                    // Doris: OPERATOR_NAME (id={plan_id}): or OPERATOR_NAME(id={plan_id}):
                    // Note: operator_name might be "OPERATOR_NAME (id=0)" or just "OPERATOR_NAME"
                    let pure_op_name = if let Some(pos) = operator_name.find(" (id=") {
                        &operator_name[..pos]
                    } else if let Some(pos) = operator_name.find("(id=") {
                        &operator_name[..pos]
                    } else if let Some(pos) = operator_name.find(" (plan_node_id=") {
                        &operator_name[..pos]
                    } else {
                        operator_name
                    };

                    // Match if line contains the pure operator name and the id
                    // Also ensure it's an operator header (ends with :)
                    let has_operator_name = trimmed.contains(pure_op_name);
                    let has_id = trimmed.contains(&format!("plan_node_id={}", plan_id))
                        || trimmed.contains(&format!("id={}", plan_id));
                    let is_header = trimmed.ends_with(':') || Self::is_operator_header(trimmed);

                    has_operator_name && has_id && is_header
                } else {
                    trimmed.starts_with(operator_name) && Self::is_operator_header(trimmed)
                };

                if is_match {
                    in_operator = true;
                    base_indent = Self::get_indent(line);
                    result.push(line);
                }
            } else {
                let current_indent = Self::get_indent(line);

                if !trimmed.is_empty() && current_indent <= base_indent {
                    if Self::is_operator_header(trimmed) {
                        break;
                    }
                }

                if current_indent > base_indent || trimmed.is_empty() {
                    result.push(line);
                } else {
                    break;
                }
            }
        }

        result.join("\n")
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
    fn test_is_operator_header() {
        assert!(OperatorParser::is_operator_header("OLAP_SCAN (plan_node_id=0):"));
        assert!(OperatorParser::is_operator_header("HASH_JOIN (plan_node_id=5):"));
        assert!(!OperatorParser::is_operator_header("- OperatorTotalTime: 100ms"));
        assert!(!OperatorParser::is_operator_header("CommonMetrics:"));
    }

    #[test]
    fn test_determine_node_type() {
        assert_eq!(OperatorParser::determine_node_type("OLAP_SCAN"), NodeType::OlapScan);
        assert_eq!(OperatorParser::determine_node_type("HASH_JOIN"), NodeType::HashJoin);
        assert_eq!(OperatorParser::determine_node_type("EXCHANGE"), NodeType::ExchangeSource);
        assert_eq!(OperatorParser::determine_node_type("RESULT_SINK"), NodeType::ResultSink);
    }

    #[test]
    fn test_canonical_topology_name() {
        assert_eq!(OperatorParser::canonical_topology_name("OLAP_SCAN"), "OLAP_SCAN");
        assert_eq!(OperatorParser::canonical_topology_name("HASH_JOIN_BUILD"), "HASH_JOIN");
        assert_eq!(OperatorParser::canonical_topology_name("AGGREGATE_BLOCKING"), "AGGREGATE");
    }
}
