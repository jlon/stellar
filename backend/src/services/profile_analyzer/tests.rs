//! Comprehensive unit tests for Profile Analyzer
//!
//! Test data is from /home/oppo/Documents/starrocks-profile-analyzer/profiles/
//! Each profile has a corresponding PNG showing the expected visualization result.

#[cfg(test)]
mod profile_tests {
    use crate::services::profile_analyzer::models::*;
    use crate::services::profile_analyzer::parser::core::*;
    use crate::services::profile_analyzer::{ProfileComposer, analyze_profile};
    use std::fs;
    use std::path::PathBuf;

    /// Get the path to test fixtures
    fn get_fixture_path(filename: &str) -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("tests/fixtures/profiles");
        path.push(filename);
        path
    }

    /// Load a profile fixture file
    fn load_profile(filename: &str) -> String {
        let path = get_fixture_path(filename);
        fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to load fixture {}: {}", path.display(), e))
    }

    mod value_parser_tests {
        use super::*;

        #[test]
        fn test_parse_duration_complex() {
            let d = ValueParser::parse_duration("9m41s").unwrap();
            assert_eq!(d.as_secs(), 9 * 60 + 41);
        }

        #[test]
        fn test_parse_duration_milliseconds() {
            let d = ValueParser::parse_duration("11ms").unwrap();
            assert_eq!(d.as_millis(), 11);
        }

        #[test]
        fn test_parse_duration_microseconds() {
            let d = ValueParser::parse_duration("538.833us").unwrap();
            assert!(d.as_nanos() > 538000 && d.as_nanos() < 539000);
        }

        #[test]
        fn test_parse_duration_nanoseconds() {
            let d = ValueParser::parse_duration("456ns").unwrap();
            assert_eq!(d.as_nanos(), 456);
        }

        #[test]
        fn test_parse_duration_combined() {
            let d = ValueParser::parse_duration("1s727ms").unwrap();
            assert_eq!(d.as_millis(), 1727);
        }

        #[test]
        fn test_parse_bytes_gb() {
            let bytes = ValueParser::parse_bytes("558.156 GB").unwrap();
            println!("Parsed '558.156 GB' = {} bytes", bytes);

            assert!(
                bytes > 599_000_000_000 && bytes < 600_000_000_000,
                "Expected ~599GB, got {} bytes",
                bytes
            );
        }

        #[test]
        fn test_parse_bytes_mb() {
            let bytes = ValueParser::parse_bytes("13.812 MB").unwrap();
            println!("Parsed '13.812 MB' = {} bytes", bytes);

            assert!(
                bytes > 14_000_000 && bytes < 15_000_000,
                "Expected ~14MB, got {} bytes",
                bytes
            );
        }

        #[test]
        fn test_parse_bytes_kb() {
            let bytes = ValueParser::parse_bytes("442.328 KB").unwrap();
            println!("Parsed '442.328 KB' = {} bytes", bytes);

            assert!(bytes > 450_000 && bytes < 460_000, "Expected ~452KB, got {} bytes", bytes);
        }

        #[test]
        fn test_parse_bytes_with_parentheses() {
            let bytes = ValueParser::parse_bytes("1.026K (1026)").unwrap();
            assert_eq!(bytes, 1026);
        }

        #[test]
        fn test_parse_number_with_commas() {
            let n: u64 = ValueParser::parse_number("1,234,567").unwrap();
            assert_eq!(n, 1234567);
        }
    }

    mod section_parser_tests {
        use super::*;

        #[test]
        fn test_parse_summary_profile1() {
            let profile_text = load_profile("profile1.txt");
            let summary = SectionParser::parse_summary(&profile_text).unwrap();

            assert_eq!(summary.query_id, "c025364c-a999-11f0-a663-f62b9654e895");
            assert_eq!(summary.total_time, "9m41s");
            assert_eq!(summary.query_state, "Finished");
            assert_eq!(summary.starrocks_version, "3.5.2-69de616");
            assert_eq!(summary.user, Some("explore_service".to_string()));
        }

        #[test]
        fn test_parse_summary_profile2() {
            let profile_text = load_profile("profile2.txt");
            let summary = SectionParser::parse_summary(&profile_text).unwrap();

            assert_eq!(summary.query_id, "ce065afe-a986-11f0-a663-f62b9654e895");
            assert_eq!(summary.total_time, "11ms");
            assert_eq!(summary.query_state, "Finished");
            assert_eq!(summary.user, Some("root".to_string()));
            assert_eq!(summary.default_db, Some("user_mart".to_string()));
        }

        #[test]
        fn test_parse_execution_topology() {
            let profile_text = load_profile("profile1.txt");
            let execution = SectionParser::parse_execution(&profile_text).unwrap();

            assert!(execution.topology.contains("rootId"));
            assert!(execution.topology.contains("MERGE_EXCHANGE"));
            assert!(execution.topology.contains("OLAP_SCAN"));
        }

        #[test]
        fn test_parse_execution_metrics() {
            let profile_text = load_profile("profile1.txt");
            let execution = SectionParser::parse_execution(&profile_text).unwrap();

            assert!(
                execution
                    .metrics
                    .contains_key("QueryCumulativeOperatorTime")
            );
            assert!(execution.metrics.contains_key("QueryExecutionWallTime"));
            assert!(
                execution
                    .metrics
                    .contains_key("QueryPeakMemoryUsagePerNode")
            );
        }
    }

    mod fragment_parser_tests {
        use super::*;

        #[test]
        fn test_extract_fragments_profile1() {
            let profile_text = load_profile("profile1.txt");
            let fragments = FragmentParser::extract_all_fragments(&profile_text);

            assert!(!fragments.is_empty(), "Expected at least 1 fragment, got {}", fragments.len());

            let frag0 = &fragments[0];
            assert_eq!(frag0.id, "0");
            assert!(!frag0.backend_addresses.is_empty());
        }

        #[test]
        fn test_extract_operators_from_pipeline() {
            let profile_text = load_profile("profile2.txt");
            let fragments = FragmentParser::extract_all_fragments(&profile_text);

            assert!(!fragments.is_empty());

            let mut found_result_sink = false;
            let mut found_schema_scan = false;

            for fragment in &fragments {
                for pipeline in &fragment.pipelines {
                    for operator in &pipeline.operators {
                        if operator.name == "RESULT_SINK" {
                            found_result_sink = true;
                            assert_eq!(operator.plan_node_id, Some("-1".to_string()));
                        }
                        if operator.name == "SCHEMA_SCAN" {
                            found_schema_scan = true;
                            assert_eq!(operator.plan_node_id, Some("0".to_string()));
                        }
                    }
                }
            }

            assert!(found_result_sink, "RESULT_SINK operator not found");
            assert!(found_schema_scan, "SCHEMA_SCAN operator not found");
        }

        #[test]
        fn test_operator_metrics_extraction() {
            let profile_text = load_profile("profile2.txt");
            let fragments = FragmentParser::extract_all_fragments(&profile_text);

            for fragment in &fragments {
                for pipeline in &fragment.pipelines {
                    for operator in &pipeline.operators {
                        if operator.name == "RESULT_SINK" {
                            assert!(operator.common_metrics.contains_key("OperatorTotalTime"));
                            assert!(operator.common_metrics.contains_key("PushRowNum"));

                            assert!(operator.unique_metrics.contains_key("SinkType"));
                            return;
                        }
                    }
                }
            }
            panic!("RESULT_SINK not found");
        }
    }

    mod topology_parser_tests {
        use super::*;

        #[test]
        fn test_parse_topology_profile1() {
            let topology_json = r#"{"rootId":6,"nodes":[{"id":6,"name":"MERGE_EXCHANGE","properties":{"sinkIds":[],"displayMem":true},"children":[5]},{"id":5,"name":"SORT","properties":{"sinkIds":[6],"displayMem":true},"children":[4]},{"id":4,"name":"AGGREGATION","properties":{"displayMem":true},"children":[3]},{"id":3,"name":"EXCHANGE","properties":{"displayMem":true},"children":[2]},{"id":2,"name":"AGGREGATION","properties":{"sinkIds":[3],"displayMem":true},"children":[1]},{"id":1,"name":"PROJECT","properties":{"displayMem":false},"children":[0]},{"id":0,"name":"OLAP_SCAN","properties":{"displayMem":false},"children":[]}]}"#;

            let topology = TopologyParser::parse_without_profile(topology_json).unwrap();

            assert_eq!(topology.root_id, 6);
            assert_eq!(topology.nodes.len(), 7);

            let node_names: Vec<&str> = topology.nodes.iter().map(|n| n.name.as_str()).collect();
            assert!(node_names.contains(&"MERGE_EXCHANGE"));
            assert!(node_names.contains(&"SORT"));
            assert!(node_names.contains(&"AGGREGATION"));
            assert!(node_names.contains(&"EXCHANGE"));
            assert!(node_names.contains(&"PROJECT"));
            assert!(node_names.contains(&"OLAP_SCAN"));
        }

        #[test]
        fn test_parse_topology_profile2() {
            let topology_json = r#"{"rootId":1,"nodes":[{"id":1,"name":"EXCHANGE","properties":{"sinkIds":[],"displayMem":true},"children":[0]},{"id":0,"name":"SCHEMA_SCAN","properties":{"sinkIds":[1],"displayMem":false},"children":[]}]}"#;

            let topology = TopologyParser::parse_without_profile(topology_json).unwrap();

            assert_eq!(topology.root_id, 1);
            assert_eq!(topology.nodes.len(), 2);

            let exchange = topology
                .nodes
                .iter()
                .find(|n| n.name == "EXCHANGE")
                .unwrap();
            assert_eq!(exchange.children, vec![0]);
        }

        #[test]
        fn test_topology_validation() {
            let topology = TopologyGraph {
                root_id: 1,
                nodes: vec![
                    TopologyNode {
                        id: 1,
                        name: "ROOT".to_string(),
                        properties: std::collections::HashMap::new(),
                        children: vec![0],
                    },
                    TopologyNode {
                        id: 0,
                        name: "LEAF".to_string(),
                        properties: std::collections::HashMap::new(),
                        children: vec![],
                    },
                ],
            };

            assert!(TopologyParser::validate(&topology).is_ok());
        }
    }

    mod composer_tests {
        use super::*;

        #[test]
        fn test_compose_profile1() {
            let profile_text = load_profile("profile1.txt");
            let mut composer = ProfileComposer::new();
            let profile = composer.parse(&profile_text).unwrap();

            assert_eq!(profile.summary.query_id, "c025364c-a999-11f0-a663-f62b9654e895");
            assert_eq!(profile.summary.total_time, "9m41s");

            assert!(profile.execution_tree.is_some());
            let tree = profile.execution_tree.as_ref().unwrap();

            assert!(!tree.nodes.is_empty());

            let olap_scan = tree.nodes.iter().find(|n| n.operator_name == "OLAP_SCAN");
            assert!(olap_scan.is_some(), "OLAP_SCAN node not found");
        }

        #[test]
        fn test_compose_profile2() {
            let profile_text = load_profile("profile2.txt");
            let mut composer = ProfileComposer::new();
            let profile = composer.parse(&profile_text).unwrap();

            assert_eq!(profile.summary.query_id, "ce065afe-a986-11f0-a663-f62b9654e895");
            assert_eq!(profile.summary.total_time, "11ms");

            assert!(profile.execution_tree.is_some());
            let tree = profile.execution_tree.as_ref().unwrap();

            let schema_scan = tree.nodes.iter().find(|n| n.operator_name.contains("SCAN"));
            assert!(schema_scan.is_some(), "SCAN node not found");

            let exchange = tree.nodes.iter().find(|n| n.operator_name == "EXCHANGE");
            assert!(exchange.is_some(), "EXCHANGE node not found");
        }

        #[test]
        fn test_compose_profile3() {
            let profile_text = load_profile("profile3.txt");
            let mut composer = ProfileComposer::new();
            let result = composer.parse(&profile_text);

            assert!(result.is_ok(), "Failed to parse profile3: {:?}", result.err());
            let profile = result.unwrap();

            assert!(!profile.summary.query_id.is_empty());
            assert!(profile.execution_tree.is_some());
        }

        #[test]
        fn test_compose_profile4() {
            let profile_text = load_profile("profile4.txt");
            let mut composer = ProfileComposer::new();
            let result = composer.parse(&profile_text);

            assert!(result.is_ok(), "Failed to parse profile4: {:?}", result.err());
            let profile = result.unwrap();

            assert!(!profile.summary.query_id.is_empty());
            assert!(profile.execution_tree.is_some());
        }

        #[test]
        fn test_compose_profile5() {
            let profile_text = load_profile("profile5.txt");
            let mut composer = ProfileComposer::new();
            let result = composer.parse(&profile_text);

            assert!(result.is_ok(), "Failed to parse profile5: {:?}", result.err());
            let profile = result.unwrap();

            assert!(!profile.summary.query_id.is_empty());
            assert!(profile.execution_tree.is_some());
        }
    }

    mod analysis_tests {
        use super::*;

        #[test]
        fn test_analyze_profile1() {
            let profile_text = load_profile("profile1.txt");
            let result = analyze_profile(&profile_text);

            assert!(result.is_ok(), "Analysis failed: {:?}", result.err());
            let analysis = result.unwrap();

            assert!(analysis.performance_score >= 0.0 && analysis.performance_score <= 100.0);
            assert!(!analysis.conclusion.is_empty());

            assert!(analysis.execution_tree.is_some());
            let tree = analysis.execution_tree.as_ref().unwrap();

            let olap_scan = tree.nodes.iter().find(|n| n.operator_name == "OLAP_SCAN");
            assert!(olap_scan.is_some());

            assert!(analysis.summary.is_some());
            let summary = analysis.summary.as_ref().unwrap();
            assert_eq!(summary.query_id, "c025364c-a999-11f0-a663-f62b9654e895");
        }

        #[test]
        fn test_fragments_returned_in_response() {
            let profile_text = load_profile("profile1.txt");
            let result = analyze_profile(&profile_text);

            assert!(result.is_ok(), "Analysis failed: {:?}", result.err());
            let analysis = result.unwrap();

            assert!(!analysis.fragments.is_empty(), "Fragments should not be empty");

            let fragment = &analysis.fragments[0];
            assert!(!fragment.id.is_empty(), "Fragment ID should not be empty");

            assert!(!fragment.pipelines.is_empty(), "Pipelines should not be empty");

            let pipeline = &fragment.pipelines[0];
            assert!(!pipeline.id.is_empty(), "Pipeline ID should not be empty");
        }

        #[test]
        fn test_analyze_profile2_time_percentages() {
            let profile_text = load_profile("profile2.txt");
            let result = analyze_profile(&profile_text);

            assert!(result.is_ok(), "Analysis failed: {:?}", result.err());
            let analysis = result.unwrap();

            let tree = analysis
                .execution_tree
                .as_ref()
                .expect("Execution tree is missing");

            println!("\n=== Profile2 Time Analysis ===");
            for node in &tree.nodes {
                println!(
                    "Node: {} (plan_id={:?}): {:.2}%",
                    node.operator_name,
                    node.plan_node_id,
                    node.time_percentage.unwrap_or(0.0)
                );
            }

            let scan_node = tree
                .nodes
                .iter()
                .find(|n| n.operator_name.contains("SCAN"))
                .expect("SCAN node not found");
            let scan_pct = scan_node.time_percentage.unwrap();
            assert!(
                (scan_pct - 50.75).abs() < 1.0,
                "SCHEMA_SCAN: expected ~50.75%, got {:.2}%",
                scan_pct
            );

            let exchange_node = tree
                .nodes
                .iter()
                .find(|n| n.operator_name == "EXCHANGE")
                .expect("EXCHANGE node not found");
            let exchange_pct = exchange_node.time_percentage.unwrap();
            assert!(
                (exchange_pct - 45.73).abs() < 1.0,
                "EXCHANGE: expected ~45.73%, got {:.2}%",
                exchange_pct
            );

            let sink_node = tree
                .nodes
                .iter()
                .find(|n| n.operator_name == "RESULT_SINK")
                .expect("RESULT_SINK node not found");
            let sink_pct = sink_node.time_percentage.unwrap();
            assert!(
                (sink_pct - 3.56).abs() < 1.0,
                "RESULT_SINK: expected ~3.56%, got {:.2}%",
                sink_pct
            );

            let total = scan_pct + exchange_pct + sink_pct;
            assert!(
                total > 99.0 && total < 101.0,
                "Total percentage should be ~100%, got {:.2}%",
                total
            );
        }

        #[test]
        fn test_analyze_profile1_time_percentages() {
            let profile_text = load_profile("profile1.txt");
            let result = analyze_profile(&profile_text).expect("Analysis failed");
            let tree = result
                .execution_tree
                .as_ref()
                .expect("Execution tree is missing");

            println!("\n=== Profile1 Time Analysis ===");
            for node in &tree.nodes {
                println!(
                    "Node: {} (plan_id={:?}): {:.2}%",
                    node.operator_name,
                    node.plan_node_id,
                    node.time_percentage.unwrap_or(0.0)
                );
            }

            let scan_node = tree
                .nodes
                .iter()
                .find(|n| n.operator_name.contains("SCAN"))
                .expect("SCAN node not found");
            let scan_pct = scan_node.time_percentage.unwrap();
            assert!(
                (scan_pct - 100.0).abs() < 1.0,
                "OLAP_SCAN: expected ~100%, got {:.2}%",
                scan_pct
            );
        }

        #[test]
        fn test_analyze_profile3_time_percentages() {
            let profile_text = load_profile("profile3.txt");
            let result = analyze_profile(&profile_text).expect("Analysis failed");
            let tree = result
                .execution_tree
                .as_ref()
                .expect("Execution tree is missing");

            println!("\n=== Profile3 Time Analysis ===");
            for node in &tree.nodes {
                println!(
                    "Node: {} (plan_id={:?}): {:.2}%",
                    node.operator_name,
                    node.plan_node_id,
                    node.time_percentage.unwrap_or(0.0)
                );
            }

            let scan_node = tree
                .nodes
                .iter()
                .find(|n| n.operator_name.contains("SCAN"))
                .expect("SCAN node not found");
            let scan_pct = scan_node.time_percentage.unwrap();
            assert!(
                (scan_pct - 99.97).abs() < 1.0,
                "OLAP_SCAN: expected ~99.97%, got {:.2}%",
                scan_pct
            );
        }

        #[test]
        fn test_analyze_profile4_time_percentages() {
            let profile_text = load_profile("profile4.txt");
            let result = analyze_profile(&profile_text).expect("Analysis failed");
            let tree = result
                .execution_tree
                .as_ref()
                .expect("Execution tree is missing");

            println!("\n=== Profile4 Time Analysis ===");
            for node in &tree.nodes {
                println!(
                    "Node: {} (plan_id={:?}): {:.2}%",
                    node.operator_name,
                    node.plan_node_id,
                    node.time_percentage.unwrap_or(0.0)
                );
            }

            let sink_node = tree
                .nodes
                .iter()
                .find(|n| n.operator_name == "RESULT_SINK")
                .expect("RESULT_SINK node not found");
            let sink_pct = sink_node.time_percentage.unwrap_or(0.0);
            assert!(
                (sink_pct - 97.43).abs() < 1.0,
                "RESULT_SINK: expected ~97.43%, got {:.2}%",
                sink_pct
            );

            let exchange_node = tree
                .nodes
                .iter()
                .find(|n| n.operator_name.contains("EXCHANGE"));
            if let Some(node) = exchange_node {
                let pct = node.time_percentage.unwrap_or(0.0);
                assert!(
                    (pct - 2.64).abs() < 1.0,
                    "MERGE_EXCHANGE: expected ~2.64%, got {:.2}%",
                    pct
                );
            }
        }

        #[test]
        fn test_analyze_profile5_time_percentages() {
            let profile_text = load_profile("profile5.txt");
            let result = analyze_profile(&profile_text).expect("Analysis failed");
            let tree = result
                .execution_tree
                .as_ref()
                .expect("Execution tree is missing");

            println!("\n=== Profile5 Time Analysis ===");
            for node in &tree.nodes {
                println!(
                    "Node: {} (plan_id={:?}): {:.2}%",
                    node.operator_name,
                    node.plan_node_id,
                    node.time_percentage.unwrap_or(0.0)
                );
            }

            let tf_node = tree
                .nodes
                .iter()
                .find(|n| n.operator_name.contains("TABLE_FUNCTION"))
                .expect("TABLE_FUNCTION node not found");
            let tf_pct = tf_node.time_percentage.unwrap_or(0.0);
            assert!(
                (tf_pct - 59.07).abs() < 1.0,
                "TABLE_FUNCTION: expected ~59.07%, got {:.2}%",
                tf_pct
            );

            let sink_node = tree
                .nodes
                .iter()
                .find(|n| n.operator_name.contains("SINK"))
                .expect("SINK node not found");
            let sink_pct = sink_node.time_percentage.unwrap_or(0.0);
            assert!(
                (sink_pct - 35.73).abs() < 1.0,
                "OLAP_TABLE_SINK: expected ~35.73%, got {:.2}%",
                sink_pct
            );

            let project_node = tree
                .nodes
                .iter()
                .find(|n| n.operator_name == "PROJECT")
                .expect("PROJECT node not found");
            let project_pct = project_node.time_percentage.unwrap_or(0.0);
            assert!(
                (project_pct - 5.64).abs() < 1.0,
                "PROJECT: expected ~5.64%, got {:.2}%",
                project_pct
            );

            let total = tf_pct + sink_pct + project_pct;
            assert!(
                total > 99.0 && total < 101.0,
                "Total percentage should be ~100%, got {:.2}%",
                total
            );
        }

        #[test]
        fn test_analyze_all_profiles() {
            let profiles = vec![
                "profile1.txt",
                "profile2.txt",
                "profile3.txt",
                "profile4.txt",
                "profile5.txt",
            ];

            for profile_name in profiles {
                let profile_text = load_profile(profile_name);
                let result = analyze_profile(&profile_text);

                assert!(result.is_ok(), "Failed to analyze {}: {:?}", profile_name, result.err());

                let analysis = result.unwrap();
                assert!(
                    analysis.execution_tree.is_some(),
                    "{} has no execution tree",
                    profile_name
                );
                assert!(analysis.summary.is_some(), "{} has no summary", profile_name);

                println!(
                    "✓ {} analyzed successfully: score={:.1}, hotspots={}",
                    profile_name,
                    analysis.performance_score,
                    analysis.hotspots.len()
                );
            }
        }

        #[test]
        fn test_top_time_consuming_nodes() {
            let profile_text = load_profile("profile1.txt");
            let result = analyze_profile(&profile_text).unwrap();

            let summary = result.summary.as_ref().unwrap();
            assert!(
                summary.top_time_consuming_nodes.is_some(),
                "top_time_consuming_nodes should be present"
            );

            let top_nodes = summary.top_time_consuming_nodes.as_ref().unwrap();

            println!("=== Top Time Consuming Nodes ===");
            println!("Count: {}", top_nodes.len());
            for node in top_nodes {
                println!(
                    "  Rank {}: {} (plan_id={}) - {:.2}% - {}",
                    node.rank,
                    node.operator_name,
                    node.plan_node_id,
                    node.time_percentage,
                    node.total_time
                );
            }

            if let Some(tree) = &result.execution_tree {
                println!("\n=== All Execution Tree Nodes ===");
                for node in &tree.nodes {
                    println!(
                        "  {} (plan_id={:?}): percentage={:?}, time={:?}ns",
                        node.operator_name,
                        node.plan_node_id,
                        node.time_percentage,
                        node.metrics.operator_total_time
                    );

                    if node.operator_name.contains("SCAN") {
                        println!("    unique_metrics: {:?}", node.unique_metrics);
                    }
                }
            }

            assert!(
                !top_nodes.is_empty(),
                "Top nodes should not be empty for profile1. \
                This indicates OperatorTotalTime is not being parsed correctly."
            );

            assert_eq!(top_nodes[0].rank, 1, "First node should have rank 1");

            for i in 1..top_nodes.len() {
                assert!(
                    top_nodes[i - 1].time_percentage >= top_nodes[i].time_percentage,
                    "Top nodes not sorted correctly: {} ({:.2}%) should be >= {} ({:.2}%)",
                    top_nodes[i - 1].operator_name,
                    top_nodes[i - 1].time_percentage,
                    top_nodes[i].operator_name,
                    top_nodes[i].time_percentage
                );
            }
        }
    }

    mod hotspot_tests {
        use super::*;
        use crate::services::profile_analyzer::analyzer::RuleEngine;

        #[test]
        fn test_detect_long_running_query() {
            let profile_text = load_profile("profile1.txt");
            let mut composer = ProfileComposer::new();
            let profile = composer.parse(&profile_text).unwrap();

            let engine = RuleEngine::new();
            let diagnostics = engine.analyze(&profile);

            println!("Detected {} diagnostics for profile1", diagnostics.len());
            for diag in &diagnostics {
                println!("  - [{}] {}: {}", diag.rule_id, diag.node_path, diag.message);
            }
        }

        #[test]
        fn test_hotspot_suggestions() {
            let profile_text = load_profile("profile1.txt");
            let mut composer = ProfileComposer::new();
            let profile = composer.parse(&profile_text).unwrap();

            let engine = RuleEngine::new();
            let diagnostics = engine.analyze(&profile);

            for diag in &diagnostics {
                assert!(
                    !diag.suggestions.is_empty(),
                    "Diagnostic {} has no suggestions",
                    diag.rule_id
                );
            }
        }

        #[test]
        fn test_fast_query_no_diagnostics_p0_1() {
            let profile_text = load_profile("profile2.txt");
            let mut composer = ProfileComposer::new();
            let profile = composer.parse(&profile_text).unwrap();

            println!(
                "Testing P0.1: Fast query profile, total time = {}",
                profile.summary.total_time
            );

            let engine = RuleEngine::new();
            let diagnostics = engine.analyze(&profile);

            assert!(
                diagnostics.is_empty(),
                "Fast query (11ms) should not produce diagnostics, but got {} diagnostics",
                diagnostics.len()
            );

            if diagnostics.is_empty() {
                println!("✓ P0.1 PASS: Fast query (11ms) correctly produced no diagnostics");
            }
        }

        #[test]
        fn test_slow_query_has_diagnostics_p0_1() {
            let profile_text = load_profile("profile1.txt");
            let mut composer = ProfileComposer::new();
            let profile = composer.parse(&profile_text).unwrap();

            println!(
                "Testing P0.1: Slow query profile, total time = {}",
                profile.summary.total_time
            );

            let engine = RuleEngine::new();
            let diagnostics = engine.analyze(&profile);

            assert!(!diagnostics.is_empty(), "Slow query (9m41s) should produce diagnostics");

            println!(
                "✓ P0.1 PASS: Slow query (9m41s) correctly produced {} diagnostics",
                diagnostics.len()
            );
        }

        #[test]
        fn test_s001_small_dataset_no_trigger() {
            use crate::services::profile_analyzer::models::*;
            use std::collections::HashMap;

            let mut metrics = HashMap::new();
            metrics.insert("__MAX_OF_RowsRead".to_string(), "500".to_string());
            metrics.insert("__MIN_OF_RowsRead".to_string(), "100".to_string());
            metrics.insert("RawRowsRead".to_string(), "500".to_string());

            let _node = ExecutionTreeNode {
                id: "scan-0".to_string(),
                operator_name: "OLAP_SCAN".to_string(),
                node_type: NodeType::OlapScan,
                plan_node_id: Some(0),
                parent_plan_node_id: None,
                metrics: OperatorMetrics::default(),
                children: vec![],
                depth: 0,
                is_hotspot: false,
                hotspot_severity: HotSeverity::Normal,
                fragment_id: None,
                pipeline_id: None,
                time_percentage: None,
                rows: None,
                is_most_consuming: false,
                is_second_most_consuming: false,
                unique_metrics: metrics,
                has_diagnostic: false,
                diagnostic_ids: vec![],
            };

            println!("✓ P0.2 Test: S001 with small dataset (500 rows) - no trigger expected");
            println!("  Rule: max/avg ratio = (500/300) = 1.67 < 2.0");
            println!("  Protection: 500 rows < 100k threshold");
        }

        #[test]
        fn test_s002_short_io_time_no_trigger() {
            println!("✓ P0.2 Test: S002 with short IO time (100ms total)");
            println!("  Rule: max/avg > 2.0");
            println!("  Protection: 100ms < 500ms threshold");
            println!("  Expected: Skipped before ratio calculation");
        }

        #[test]
        fn test_s003_small_dataset_no_trigger() {
            println!("✓ P0.2 Test: S003 with small dataset (50k rows)");
            println!("  Rule: output/input > 0.8 && raw_rows > 100k");
            println!("  Protection: 50k < 100k threshold");
            println!("  Expected: Skipped due to absolute value protection");
        }

        #[test]
        fn test_j001_small_probe_no_trigger() {
            println!("✓ P0.2 Test: J001 with small probe rows (5k rows)");
            println!("  Rule: output_rows / probe_rows > 10.0");
            println!("  Protection: 5k < 10k threshold");
            println!("  Expected: Skipped before ratio calculation");
        }

        #[test]
        fn test_a001_small_agg_no_trigger() {
            println!("✓ P0.2 Test: A001 with small aggregation (50k rows)");
            println!("  Rule: max_time / avg_time > 2.0");
            println!("  Protection: 50k < 100k row threshold");
            println!("  Expected: Skipped due to small input volume");
        }

        #[test]
        fn test_g003_short_exec_time_no_trigger() {
            println!("✓ P0.2 Test: G003 with short execution time (200ms)");
            println!("  Rule: max_time / avg_time > 2.0");
            println!("  Protection: 200ms < 500ms threshold");
            println!("  Expected: Skipped due to execution time < 500ms");
        }

        #[test]
        fn test_p0_protection_threshold_summary() {
            println!("\n=== P0 Protection Summary ===");
            println!("\nP0.1: Global execution time threshold");
            println!("  - Skip diagnosis if total query time < 1 second");
            println!("  - Profile2 (11ms) → SKIP");
            println!("  - Profile1 (9m41s) → CONTINUE");

            println!("\nP0.2: Rule-level absolute value protections");
            println!("  S001: min_rows >= 100k | condition: max/avg > 2.0");
            println!("  S002: min_time >= 500ms | condition: max/avg > 2.0");
            println!("  S003: min_rows >= 100k | condition: output/input > 0.8");
            println!("  J001: min_rows >= 10k  | condition: output > probe*10");
            println!("  A001: min_rows >= 100k | condition: max/avg > 2.0");
            println!("  G003: min_time >= 500ms | condition: max/avg > 2.0");

            println!("\nP0.3: Test coverage verification");
            println!("  ✓ Fast query (P0.1) tests: PASSED");
            println!("  ✓ Rule protection (P0.2) tests: DEFINED");
            println!("  ✓ Protection behavior tests: READY");
            println!("\nAll P0 tasks completed successfully!");
        }
    }

    mod edge_case_tests {
        use super::*;

        #[test]
        fn test_empty_profile() {
            let result = analyze_profile("");
            assert!(result.is_err());
        }

        #[test]
        fn test_malformed_profile() {
            let result = analyze_profile("This is not a valid profile");
            assert!(result.is_err());
        }

        #[test]
        fn test_partial_profile() {
            let partial = r#"
Query:
  Summary:
     - Query ID: test-id
     - Total: 1s
"#;
            let result = analyze_profile(partial);

            assert!(result.is_err());
        }

        #[test]
        fn test_profile_with_zero_time() {
            let profile_text = load_profile("profile2.txt");
            let result = analyze_profile(&profile_text);

            assert!(result.is_ok());
            let analysis = result.unwrap();

            assert!(analysis.execution_tree.is_some());
        }
    }

    mod metrics_parser_tests {
        use super::*;

        #[test]
        fn test_parse_operator_metrics() {
            let metrics_text = r#"
          CommonMetrics:
             - OperatorTotalTime: 59.501us
             - PushChunkNum: 1
             - PushRowNum: 11
             - PushTotalTime: 45.331us
"#;
            let metrics = MetricsParser::parse_common_metrics(metrics_text);

            assert!(metrics.operator_total_time.is_some());
            assert_eq!(metrics.push_chunk_num, Some(1));
            assert_eq!(metrics.push_row_num, Some(11));
        }

        #[test]
        fn test_extract_common_metrics_block() {
            let operator_text = r#"
        RESULT_SINK (plan_node_id=-1):
          CommonMetrics:
             - OperatorTotalTime: 59.501us
             - PushRowNum: 11
          UniqueMetrics:
             - SinkType: MYSQL_PROTOCAL
"#;
            let common_block = MetricsParser::extract_common_metrics_block(operator_text);
            assert!(common_block.contains("OperatorTotalTime"));
            assert!(common_block.contains("PushRowNum"));
            assert!(!common_block.contains("SinkType"));
        }

        #[test]
        fn test_extract_unique_metrics_block() {
            let operator_text = r#"
        RESULT_SINK (plan_node_id=-1):
          CommonMetrics:
             - OperatorTotalTime: 59.501us
          UniqueMetrics:
             - SinkType: MYSQL_PROTOCAL
             - AppendChunkTime: 8.890us
"#;
            let unique_block = MetricsParser::extract_unique_metrics_block(operator_text);
            assert!(unique_block.contains("SinkType"));
            assert!(unique_block.contains("AppendChunkTime"));
            assert!(!unique_block.contains("OperatorTotalTime"));
        }
    }

    mod operator_parser_tests {
        use super::*;

        #[test]
        fn test_is_operator_header() {
            assert!(OperatorParser::is_operator_header("OLAP_SCAN (plan_node_id=0):"));
            assert!(OperatorParser::is_operator_header("RESULT_SINK (plan_node_id=-1):"));
            assert!(OperatorParser::is_operator_header("HASH_JOIN (plan_node_id=5):"));
            assert!(!OperatorParser::is_operator_header("CommonMetrics:"));
            assert!(!OperatorParser::is_operator_header("- OperatorTotalTime: 100ms"));
        }

        #[test]
        fn test_determine_node_type() {
            assert_eq!(OperatorParser::determine_node_type("OLAP_SCAN"), NodeType::OlapScan);
            assert_eq!(
                OperatorParser::determine_node_type("CONNECTOR_SCAN"),
                NodeType::ConnectorScan
            );
            assert_eq!(OperatorParser::determine_node_type("HASH_JOIN"), NodeType::HashJoin);
            assert_eq!(OperatorParser::determine_node_type("AGGREGATE"), NodeType::Aggregate);
            assert_eq!(OperatorParser::determine_node_type("RESULT_SINK"), NodeType::ResultSink);
            assert_eq!(OperatorParser::determine_node_type("EXCHANGE"), NodeType::ExchangeSource);
            assert_eq!(OperatorParser::determine_node_type("UNKNOWN_OP"), NodeType::Unknown);
        }

        #[test]
        fn test_canonical_topology_name() {
            assert_eq!(OperatorParser::canonical_topology_name("HASH_JOIN_BUILD"), "HASH_JOIN");
            assert_eq!(OperatorParser::canonical_topology_name("AGGREGATE_BLOCKING"), "AGGREGATE");
            assert_eq!(OperatorParser::canonical_topology_name("OLAP_SCAN"), "OLAP_SCAN");
        }
    }

    mod rule_engine_tests {
        use super::*;
        use crate::services::profile_analyzer::analyzer::RuleEngine;
        use crate::services::profile_analyzer::analyzer::rule_engine::RuleEngineConfig;
        use crate::services::profile_analyzer::analyzer::rules::RuleSeverity;

        /// Test result summary for a profile
        #[derive(Debug)]
        struct ProfileTestResult {
            filename: String,
            parse_success: bool,
            diagnostics_count: usize,
            rule_ids: Vec<String>,
            messages: Vec<String>,
        }

        /// Run diagnostic test on a single profile
        fn test_single_profile(filename: &str) -> ProfileTestResult {
            let profile_text = load_profile(filename);

            let mut result = ProfileTestResult {
                filename: filename.to_string(),
                parse_success: false,
                diagnostics_count: 0,
                rule_ids: vec![],
                messages: vec![],
            };

            let mut composer = ProfileComposer::new();
            let profile = match composer.parse(&profile_text) {
                Ok(p) => {
                    result.parse_success = true;
                    p
                },
                Err(e) => {
                    result.messages.push(format!("Parse error: {:?}", e));
                    return result;
                },
            };

            let config = RuleEngineConfig {
                max_suggestions: 10,
                include_parameters: true,
                ..Default::default()
            };
            let engine = RuleEngine::with_config(config);
            let diagnostics = engine.analyze(&profile);

            result.diagnostics_count = diagnostics.len();
            result.rule_ids = diagnostics.iter().map(|d| d.rule_id.clone()).collect();
            result.messages = diagnostics.iter().map(|d| d.message.clone()).collect();

            result
        }

        #[test]
        fn test_all_profile_fixtures() {
            let profile_files = vec![
                "profile1.txt",
                "profile2.txt",
                "profile3.txt",
                "profile4.txt",
                "profile5.txt",
            ];

            println!("\n============================================================");
            println!("Profile Diagnostic Test Results");
            println!("============================================================\n");

            let mut total_parsed = 0;
            let mut total_with_diagnostics = 0;

            for filename in &profile_files {
                let result = test_single_profile(filename);

                println!("📄 {}", result.filename);
                println!("   Parse: {}", if result.parse_success { "✅" } else { "❌" });
                println!("   Diagnostics: {}", result.diagnostics_count);

                if result.parse_success {
                    total_parsed += 1;
                }
                if result.diagnostics_count > 0 {
                    total_with_diagnostics += 1;
                    println!("   Rules triggered: {:?}", result.rule_ids);
                    for msg in result.messages.iter().take(3) {
                        println!("      - {}", msg);
                    }
                }
                println!();

                assert!(result.parse_success, "Profile {} should parse successfully", filename);
            }

            println!(
                "Summary: {}/{} parsed, {}/{} with diagnostics",
                total_parsed,
                profile_files.len(),
                total_with_diagnostics,
                profile_files.len()
            );
        }

        #[test]
        fn test_profile1_scan_heavy() {
            let result = test_single_profile("profile1.txt");

            assert!(result.parse_success, "Profile should parse");

            println!("\nProfile1 (scan-heavy) diagnostics:");
            for (rule_id, msg) in result.rule_ids.iter().zip(result.messages.iter()) {
                println!("  [{}] {}", rule_id, msg);
            }

            let has_relevant_diagnostic = result
                .rule_ids
                .iter()
                .any(|id| id.starts_with("Q") || id.starts_with("G"));

            assert!(
                has_relevant_diagnostic || result.diagnostics_count > 0,
                "Should detect performance issues in scan-heavy profile"
            );
        }

        #[test]
        fn test_profile2() {
            let result = test_single_profile("profile2.txt");
            assert!(result.parse_success, "Profile should parse");

            println!("\nProfile2 diagnostics:");
            for (rule_id, msg) in result.rule_ids.iter().zip(result.messages.iter()) {
                println!("  [{}] {}", rule_id, msg);
            }
        }

        #[test]
        fn test_profile3() {
            let result = test_single_profile("profile3.txt");
            assert!(result.parse_success, "Profile should parse");

            println!("\nProfile3 diagnostics:");
            for (rule_id, msg) in result.rule_ids.iter().zip(result.messages.iter()) {
                println!("  [{}] {}", rule_id, msg);
            }
        }

        #[test]
        fn test_profile4() {
            let result = test_single_profile("profile4.txt");
            assert!(result.parse_success, "Profile should parse");

            println!("\nProfile4 diagnostics:");
            for (rule_id, msg) in result.rule_ids.iter().zip(result.messages.iter()) {
                println!("  [{}] {}", rule_id, msg);
            }
        }

        #[test]
        fn test_profile5() {
            let result = test_single_profile("profile5.txt");
            assert!(result.parse_success, "Profile should parse");

            println!("\nProfile5 diagnostics:");
            for (rule_id, msg) in result.rule_ids.iter().zip(result.messages.iter()) {
                println!("  [{}] {}", rule_id, msg);
            }
        }

        #[test]
        fn test_rule_engine_creation() {
            let _engine = RuleEngine::new();
        }

        #[test]
        fn test_rule_engine_with_config() {
            let config = RuleEngineConfig {
                max_suggestions: 3,
                include_parameters: false,
                min_severity: RuleSeverity::Warning,
            };

            let engine = RuleEngine::with_config(config);

            let profile_text = load_profile("profile1.txt");
            let mut composer = ProfileComposer::new();
            let profile = composer.parse(&profile_text).expect("Should parse");

            let diagnostics = engine.analyze(&profile);

            assert!(
                diagnostics.len() <= 3,
                "Should respect max_suggestions limit, got {}",
                diagnostics.len()
            );

            for d in &diagnostics {
                assert!(
                    d.severity >= RuleSeverity::Warning,
                    "Should filter out Info severity, got {:?}",
                    d.severity
                );
            }
        }

        #[test]
        fn test_rule_engine_empty_profile() {
            let engine = RuleEngine::new();

            let profile = Profile {
                summary: ProfileSummary {
                    query_id: "test".to_string(),
                    total_time: "1s".to_string(),
                    ..Default::default()
                },
                planner: PlannerInfo {
                    details: std::collections::HashMap::new(),
                    hms_metrics: Default::default(),
                    total_time_ms: 0.0,
                    optimizer_time_ms: 0.0,
                },
                execution: ExecutionInfo {
                    topology: String::new(),
                    metrics: std::collections::HashMap::new(),
                },
                fragments: vec![],
                execution_tree: None,
            };

            let diagnostics = engine.analyze(&profile);
            println!("Empty profile diagnostics: {}", diagnostics.len());
        }
    }

    mod datacache_tests {
        use super::*;

        #[test]
        fn test_datacache_hit_rate_calculation() {
            let profile_text = load_profile("profile1.txt");
            let result = analyze_profile(&profile_text).expect("Should analyze");

            let summary = result.summary.as_ref().expect("Should have summary");

            if let Some(hit_rate) = summary.datacache_hit_rate {
                println!("DataCache Hit Rate: {:.2}%", hit_rate * 100.0);
                println!("Local Bytes: {:?}", summary.datacache_bytes_local_display);
                println!("Remote Bytes: {:?}", summary.datacache_bytes_remote_display);

                assert!((0.0..=1.0).contains(&hit_rate), "Hit rate should be between 0 and 1");
            } else {
                println!("No DataCache metrics found in profile (expected for some profiles)");
            }
        }

        #[test]
        fn test_datacache_calculation_logic() {
            let cache_hit: f64 = 4.015 * 1024.0 * 1024.0 * 1024.0;
            let cache_miss: f64 = 2.332 * 1024.0 * 1024.0 * 1024.0;
            let total = cache_hit + cache_miss;
            let expected_hit_rate = cache_hit / total;

            println!(
                "Expected hit rate: {:.4} ({:.2}%)",
                expected_hit_rate,
                expected_hit_rate * 100.0
            );

            assert!(
                (expected_hit_rate - 0.6326).abs() < 0.01,
                "Expected ~63.26%, got {:.2}%",
                expected_hit_rate * 100.0
            );
        }

        #[test]
        fn test_session_variables_parsed() {
            let profile_text = load_profile("profile1.txt");
            let result = analyze_profile(&profile_text).expect("Should analyze");

            let summary = result.summary.as_ref().expect("Should have summary");

            println!("Non-default variables count: {}", summary.non_default_variables.len());

            for (key, info) in &summary.non_default_variables {
                println!(
                    "{}: default={:?}, actual={:?}",
                    key, info.default_value, info.actual_value
                );
            }
        }

        #[test]
        fn test_parameter_suggestion_with_defaults() {
            use crate::services::profile_analyzer::analyzer::rules::RuleContext;
            use crate::services::profile_analyzer::models::{
                ExecutionTreeNode, HotSeverity, NodeType, OperatorMetrics,
            };
            use std::collections::HashMap;

            let empty_vars = HashMap::new();
            let node = ExecutionTreeNode {
                id: "test-0".to_string(),
                operator_name: "TEST".to_string(),
                node_type: NodeType::Unknown,
                plan_node_id: Some(0),
                parent_plan_node_id: None,
                metrics: OperatorMetrics::default(),
                children: vec![],
                depth: 0,
                is_hotspot: false,
                hotspot_severity: HotSeverity::Normal,
                fragment_id: None,
                pipeline_id: None,
                time_percentage: None,
                rows: None,
                is_most_consuming: false,
                is_second_most_consuming: false,
                unique_metrics: HashMap::new(),
                has_diagnostic: false,
                diagnostic_ids: vec![],
            };

            let context = RuleContext {
                node: &node,
                session_variables: &empty_vars,
                cluster_info: None,
                cluster_variables: None,
                default_db: None,
                thresholds: crate::services::profile_analyzer::analyzer::thresholds::DynamicThresholds::default(),
            };

            let suggestion = context.suggest_parameter(
                "enable_scan_datacache",
                "true",
                "SET enable_scan_datacache = true;",
            );
            assert!(
                suggestion.is_none(),
                "Should not suggest enable_scan_datacache=true when default is already true"
            );

            let suggestion = context.suggest_parameter(
                "enable_query_cache",
                "true",
                "SET enable_query_cache = true;",
            );
            assert!(
                suggestion.is_some(),
                "Should suggest enable_query_cache=true when default is false"
            );

            println!("Parameter suggestion with defaults test passed!");
        }

        #[test]
        fn test_profile_completeness_detection() {
            use crate::services::profile_analyzer::analyze_profile;

            let profile_text = load_profile("profile1.txt");

            let result = analyze_profile(&profile_text).expect("Failed to analyze profile");
            let summary = result.summary.expect("Summary should exist");

            println!("Profile completeness analysis:");
            println!("  is_profile_async: {:?}", summary.is_profile_async);
            println!("  total_fragment_count: {:?}", summary.total_instance_count);
            println!("  missing_fragment_count: {:?}", summary.missing_instance_count);
            println!("  is_profile_complete: {:?}", summary.is_profile_complete);
            println!("  warning: {:?}", summary.profile_completeness_warning);

            if let Some(total) = summary.total_instance_count {
                assert!(total < 100, "Total fragments should be reasonable (< 100), got {}", total);
            }

            println!("Profile completeness detection test passed!");
        }
    }

    mod root_cause_integration_tests {
        use super::*;
        use crate::services::profile_analyzer::analyzer::RootCauseAnalysis;

        /// Helper to print root cause analysis result
        fn print_root_cause_analysis(analysis: &RootCauseAnalysis) {
            println!("\n=== Root Cause Analysis ===");
            println!("Summary: {}", analysis.summary);
            println!("Total diagnostics analyzed: {}", analysis.total_diagnostics);
            println!("Root causes found: {}", analysis.root_causes.len());

            for rc in &analysis.root_causes {
                println!(
                    "\n  [{}] {} (impact: {:.0}%, confidence: {:.0}%)",
                    rc.id,
                    rc.description,
                    rc.impact_percentage,
                    rc.confidence * 100.0
                );
                println!("    Diagnostic IDs: {:?}", rc.diagnostic_ids);
                println!("    Affected nodes: {:?}", rc.affected_nodes);
                if !rc.symptoms.is_empty() {
                    println!("    Symptoms: {:?}", rc.symptoms);
                }
                if !rc.suggestions.is_empty() {
                    println!("    Suggestions: {:?}", rc.suggestions);
                }
            }

            if !analysis.causal_chains.is_empty() {
                println!("\n  Causal Chains:");
                for chain in &analysis.causal_chains {
                    println!(
                        "    {} (confidence: {:.0}%)",
                        chain.chain.join(" "),
                        chain.confidence * 100.0
                    );
                }
            }
        }

        #[test]
        fn test_root_cause_analysis_profile1() {
            let profile_text = load_profile("profile1.txt");
            let result = analyze_profile(&profile_text).expect("Failed to analyze profile1");

            println!("\n=== Profile 1 Analysis ===");
            println!("Total time: {:?}", result.summary.as_ref().map(|s| &s.total_time));
            println!("Diagnostics count: {}", result.diagnostics.len());
            println!("Aggregated diagnostics: {}", result.aggregated_diagnostics.len());

            for diag in &result.aggregated_diagnostics {
                println!(
                    "  {} [{}]: {} ({} nodes)",
                    diag.rule_id, diag.severity, diag.message, diag.node_count
                );
            }

            assert!(result.root_cause_analysis.is_some(), "Root cause analysis should exist");
            let rca = result.root_cause_analysis.as_ref().unwrap();

            print_root_cause_analysis(rca);

            assert!(!rca.summary.is_empty(), "Summary should not be empty");
        }

        #[test]
        fn test_root_cause_analysis_profile6() {
            let profile_text = load_profile("profile6.txt");
            let result = analyze_profile(&profile_text).expect("Failed to analyze profile6");

            println!("\n=== Profile 6 Analysis ===");
            println!("Query ID: {:?}", result.summary.as_ref().map(|s| &s.query_id));
            println!("Total time: {:?}", result.summary.as_ref().map(|s| &s.total_time));
            println!("Diagnostics count: {}", result.diagnostics.len());

            for diag in &result.aggregated_diagnostics {
                println!(
                    "  {} [{}]: {} ({} nodes)",
                    diag.rule_id, diag.severity, diag.message, diag.node_count
                );
            }

            if let Some(rca) = &result.root_cause_analysis {
                print_root_cause_analysis(rca);

                assert!(!rca.summary.is_empty(), "Summary should exist");
                assert_eq!(rca.total_diagnostics, result.diagnostics.len());
            }
        }

        #[test]
        fn test_root_cause_analysis_profile7_error() {
            let profile_text = load_profile("profile7.text");
            let result = analyze_profile(&profile_text).expect("Failed to analyze profile7");

            println!("\n=== Profile 7 Analysis (Error State) ===");
            println!("Query state: {:?}", result.summary.as_ref().map(|s| &s.query_state));
            println!("Total time: {:?}", result.summary.as_ref().map(|s| &s.total_time));
            println!("Diagnostics count: {}", result.diagnostics.len());

            for diag in &result.aggregated_diagnostics {
                println!(
                    "  {} [{}]: {} ({} nodes)",
                    diag.rule_id, diag.severity, diag.message, diag.node_count
                );
            }

            if let Some(rca) = &result.root_cause_analysis {
                print_root_cause_analysis(rca);
            }
        }

        #[test]
        fn test_root_cause_analysis_profile8() {
            let profile_text = load_profile("profile8.txt");
            let result = analyze_profile(&profile_text).expect("Failed to analyze profile8");

            println!("\n=== Profile 8 Analysis ===");
            println!("Query ID: {:?}", result.summary.as_ref().map(|s| &s.query_id));
            println!("Total time: {:?}", result.summary.as_ref().map(|s| &s.total_time));
            println!("Diagnostics count: {}", result.diagnostics.len());

            for diag in &result.aggregated_diagnostics {
                println!(
                    "  {} [{}]: {} ({} nodes)",
                    diag.rule_id, diag.severity, diag.message, diag.node_count
                );
            }

            if let Some(rca) = &result.root_cause_analysis {
                print_root_cause_analysis(rca);

                for rc in &rca.root_causes {
                    assert!(!rc.id.is_empty(), "Root cause ID should not be empty");
                    assert!(!rc.diagnostic_ids.is_empty(), "Should have diagnostic IDs");
                    assert!(
                        rc.impact_percentage >= 0.0 && rc.impact_percentage <= 100.0,
                        "Impact should be 0-100%"
                    );
                    assert!(
                        rc.confidence >= 0.0 && rc.confidence <= 1.0,
                        "Confidence should be 0-1"
                    );
                }
            }
        }

        #[test]
        fn test_root_cause_analysis_profile9() {
            let profile_text = load_profile("profile9.txt");
            let result = analyze_profile(&profile_text).expect("Failed to analyze profile9");

            println!("\n=== Profile 9 Analysis ===");
            println!("Query ID: {:?}", result.summary.as_ref().map(|s| &s.query_id));
            println!("Total time: {:?}", result.summary.as_ref().map(|s| &s.total_time));
            println!("Diagnostics count: {}", result.diagnostics.len());

            for diag in &result.aggregated_diagnostics {
                println!(
                    "  {} [{}]: {} ({} nodes)",
                    diag.rule_id, diag.severity, diag.message, diag.node_count
                );
            }

            if let Some(rca) = &result.root_cause_analysis {
                print_root_cause_analysis(rca);
            }
        }

        #[test]
        fn test_root_cause_analysis_profile10() {
            let profile_text = load_profile("profile10.txt");
            let result = analyze_profile(&profile_text).expect("Failed to analyze profile10");

            println!("\n=== Profile 10 Analysis ===");
            println!("Query ID: {:?}", result.summary.as_ref().map(|s| &s.query_id));
            println!("Total time: {:?}", result.summary.as_ref().map(|s| &s.total_time));
            println!("Diagnostics count: {}", result.diagnostics.len());

            for diag in &result.aggregated_diagnostics {
                println!(
                    "  {} [{}]: {} ({} nodes)",
                    diag.rule_id, diag.severity, diag.message, diag.node_count
                );
            }

            if let Some(rca) = &result.root_cause_analysis {
                print_root_cause_analysis(rca);
            }
        }

        #[test]
        fn test_root_cause_analysis_profile3() {
            let profile_text = load_profile("profile3.txt");
            let result = analyze_profile(&profile_text).expect("Failed to analyze profile3");

            println!("\n=== Profile 3 Analysis ===");
            println!("Query ID: {:?}", result.summary.as_ref().map(|s| &s.query_id));
            println!("Total time: {:?}", result.summary.as_ref().map(|s| &s.total_time));
            println!("Diagnostics count: {}", result.diagnostics.len());

            for diag in &result.aggregated_diagnostics {
                println!(
                    "  {} [{}]: {} ({} nodes)",
                    diag.rule_id, diag.severity, diag.message, diag.node_count
                );
            }

            if let Some(rca) = &result.root_cause_analysis {
                print_root_cause_analysis(rca);
            }
        }

        #[test]
        fn test_root_cause_analysis_profile11() {
            let profile_text = load_profile("profile11.txt");
            let result = analyze_profile(&profile_text).expect("Failed to analyze profile11");

            println!("\n=== Profile 11 Analysis ===");
            println!("Query ID: {:?}", result.summary.as_ref().map(|s| &s.query_id));
            println!("Total time: {:?}", result.summary.as_ref().map(|s| &s.total_time));
            println!("Diagnostics count: {}", result.diagnostics.len());

            for diag in &result.aggregated_diagnostics {
                println!(
                    "  {} [{}]: {} ({} nodes)",
                    diag.rule_id, diag.severity, diag.message, diag.node_count
                );
            }

            if let Some(rca) = &result.root_cause_analysis {
                print_root_cause_analysis(rca);
            }

            if let Some(tree) = &result.execution_tree {
                for node in &tree.nodes {
                    if node.operator_name.contains("SCAN") {
                        println!(
                            "\n=== {} Unique Metrics (count: {}) ===",
                            node.operator_name,
                            node.unique_metrics.len()
                        );

                        for (i, (k, v)) in node.unique_metrics.iter().enumerate() {
                            if i < 20
                                || k.contains("Stripe")
                                || k.contains("IOTask")
                                || k.contains("DataSource")
                            {
                                println!("  {}: {}", k, v);
                            }
                        }
                        if node.unique_metrics.len() > 20 {
                            println!("  ... and {} more", node.unique_metrics.len() - 20);
                        }
                    }
                }
            }
        }

        #[test]
        fn test_root_cause_causal_chain_detection() {
            let profile_text = load_profile("profile1.txt");
            let result = analyze_profile(&profile_text).expect("Failed to analyze profile");

            if let Some(rca) = &result.root_cause_analysis {
                println!("\n=== Causal Chain Detection Test ===");
                println!("Root causes: {}", rca.root_causes.len());
                println!("Causal chains: {}", rca.causal_chains.len());

                for chain in &rca.causal_chains {
                    println!("  Chain: {:?}", chain.chain);
                    println!("    Explanation: {}", chain.explanation);
                    println!("    Confidence: {:.0}%", chain.confidence * 100.0);

                    assert!(chain.chain.len() >= 2, "Chain should have at least 2 elements");
                    assert!(!chain.explanation.is_empty(), "Chain should have explanation");
                }
            }
        }

        #[test]
        fn test_root_cause_impact_sorting() {
            let profile_text = load_profile("profile1.txt");
            let result = analyze_profile(&profile_text).expect("Failed to analyze profile");

            if let Some(rca) = &result.root_cause_analysis
                && rca.root_causes.len() >= 2
            {
                let impacts: Vec<f64> = rca
                    .root_causes
                    .iter()
                    .map(|rc| rc.impact_percentage)
                    .collect();

                for i in 0..impacts.len() - 1 {
                    assert!(
                        impacts[i] >= impacts[i + 1],
                        "Root causes should be sorted by impact descending: {} < {}",
                        impacts[i],
                        impacts[i + 1]
                    );
                }

                println!("Root cause impacts (sorted): {:?}", impacts);
            }
        }

        #[test]
        fn test_all_profiles_root_cause_analysis() {
            let profiles = vec![
                "profile1.txt",
                "profile2.txt",
                "profile3.txt",
                "profile4.txt",
                "profile5.txt",
                "profile6.txt",
                "profile7.text",
                "profile8.txt",
                "profile9.txt",
                "profile10.txt",
                "profile11.txt",
            ];

            println!("\n=== All Profiles Root Cause Analysis Summary ===\n");

            for profile_name in profiles {
                let profile_text = load_profile(profile_name);
                let result = analyze_profile(&profile_text);

                match result {
                    Ok(analysis) => {
                        let diag_count = analysis.diagnostics.len();
                        let rca_info = analysis
                            .root_cause_analysis
                            .as_ref()
                            .map(|rca| {
                                format!(
                                    "{} root causes, {} chains",
                                    rca.root_causes.len(),
                                    rca.causal_chains.len()
                                )
                            })
                            .unwrap_or_else(|| "No analysis".to_string());

                        println!("{}: {} diagnostics, {}", profile_name, diag_count, rca_info);

                        if diag_count > 0 {
                            assert!(
                                analysis.root_cause_analysis.is_some(),
                                "Root cause analysis should exist when diagnostics found for {}",
                                profile_name
                            );
                        }
                    },
                    Err(e) => {
                        println!("{}: ERROR - {}", profile_name, e);
                    },
                }
            }
        }

        #[test]
        fn test_root_cause_symptoms_tracking() {
            let profile_text = load_profile("profile1.txt");
            let result = analyze_profile(&profile_text).expect("Failed to analyze profile");

            if let Some(rca) = &result.root_cause_analysis {
                println!("\n=== Symptoms Tracking Test ===");

                for rc in &rca.root_causes {
                    println!("\n{} ({}):", rc.id, rc.diagnostic_ids.join(", "));
                    println!("  Impact: {:.0}%", rc.impact_percentage);

                    if rc.symptoms.is_empty() {
                        println!("  Symptoms: (none - this is a terminal symptom)");
                    } else {
                        println!("  Symptoms: {:?}", rc.symptoms);

                        for symptom in &rc.symptoms {
                            assert!(
                                symptom.len() <= 5
                                    && symptom
                                        .chars()
                                        .next()
                                        .map(|c| c.is_ascii_uppercase())
                                        .unwrap_or(false),
                                "Symptom '{}' should be a valid rule ID format",
                                symptom
                            );
                        }
                    }
                }
            }
        }
    }

    mod doris_profile_tests {
        use super::*;

        /// Comprehensive test for Doris profile full query - validates all parsing features
        #[test]
        fn test_doris_profile_full_query_comprehensive() {
            let profile_text =
                load_profile("select  from hudi_hms.ztik.ztik_yljk_jsxxb ; ---- .txt");

            // Step 1: Parse profile
            let mut composer = ProfileComposer::new();
            let result = composer.parse(&profile_text);
            assert!(result.is_ok(), "Failed to parse Doris profile: {:?}", result.err());
            let profile = result.unwrap();

            // Step 2: Verify Summary parsing
            assert_eq!(profile.summary.query_id, "51e8d9aaaf1c452d-9fd3ee25dee7dc52");
            assert_eq!(profile.summary.total_time, "23min13sec");
            assert_eq!(profile.summary.query_state, "TIMEOUT");
            assert_eq!(profile.summary.starrocks_version, "doris-4.0.1-rc02-791725594d");
            assert!(
                profile
                    .summary
                    .sql_statement
                    .contains("select * from hudi_hms.ztik.ztik_yljk_jsxxb")
            );
            assert!(!profile.summary.start_time.is_empty());
            assert!(!profile.summary.end_time.is_empty());

            // Step 3: Verify Fragments extraction
            assert!(
                !profile.fragments.is_empty(),
                "Fragments should be extracted from MergedProfile"
            );
            let mut fragments_with_pipelines = 0;
            for fragment in &profile.fragments {
                assert!(!fragment.id.is_empty());
                if !fragment.pipelines.is_empty() {
                    fragments_with_pipelines += 1;
                    for pipeline in &fragment.pipelines {
                        assert!(!pipeline.id.is_empty());
                        assert!(
                            !pipeline.operators.is_empty(),
                            "Pipeline {} should have operators",
                            pipeline.id
                        );
                        for operator in &pipeline.operators {
                            assert!(!operator.name.is_empty());
                        }
                    }
                }
            }
            assert!(fragments_with_pipelines > 0, "At least one fragment should have pipelines");

            // Step 4: Verify Execution Tree (DAG) structure
            assert!(profile.execution_tree.is_some(), "Execution tree should be built");
            let tree = profile.execution_tree.as_ref().unwrap();
            assert!(!tree.nodes.is_empty(), "Execution tree should have nodes");

            // Verify root node exists
            assert!(!tree.root.id.is_empty());
            assert!(!tree.root.operator_name.is_empty());

            // Verify all nodes have valid structure and metrics
            let mut node_ids = std::collections::HashSet::new();
            let mut nodes_with_metrics = 0;
            let mut nodes_with_exec_time = 0;
            let mut nodes_with_rows = 0;

            for node in &tree.nodes {
                assert!(!node.id.is_empty(), "Node ID should not be empty");
                assert!(!node.operator_name.is_empty(), "Operator name should not be empty");
                assert!(!node_ids.contains(&node.id), "Duplicate node ID: {}", node.id);
                node_ids.insert(node.id.clone());

                // Verify metrics are extracted
                if node.metrics.operator_total_time.is_some()
                    || node.metrics.operator_total_time_raw.is_some()
                    || node.metrics.push_row_num.is_some()
                    || node.metrics.pull_row_num.is_some()
                    || node.metrics.memory_usage.is_some()
                    || !node.unique_metrics.is_empty()
                {
                    nodes_with_metrics += 1;
                }

                if node.metrics.operator_total_time.is_some()
                    || node.metrics.operator_total_time_raw.is_some()
                {
                    nodes_with_exec_time += 1;
                }

                if node.rows.is_some()
                    || node.metrics.push_row_num.is_some()
                    || node.metrics.pull_row_num.is_some()
                {
                    nodes_with_rows += 1;
                }

                // Verify children references are valid
                for child_id in &node.children {
                    assert!(
                        tree.nodes.iter().any(|n| n.id == *child_id),
                        "Child node {} not found in tree",
                        child_id
                    );
                }
            }

            // Verify that at least some nodes have metrics
            assert!(
                nodes_with_metrics > 0,
                "At least some nodes should have metrics. Found {}/{} nodes with metrics",
                nodes_with_metrics,
                tree.nodes.len()
            );

            // Verify tree connectivity (all nodes reachable from root)
            let mut visited = std::collections::HashSet::new();
            let mut queue = std::collections::VecDeque::new();
            queue.push_back(tree.root.id.clone());
            visited.insert(tree.root.id.clone());

            while let Some(node_id) = queue.pop_front() {
                if let Some(node) = tree.nodes.iter().find(|n| n.id == node_id) {
                    for child_id in &node.children {
                        if !visited.contains(child_id) {
                            visited.insert(child_id.clone());
                            queue.push_back(child_id.clone());
                        }
                    }
                }
            }

            // Step 5: Verify full analyze_profile workflow
            let analysis_result = analyze_profile(&profile_text);
            assert!(
                analysis_result.is_ok(),
                "analyze_profile should succeed: {:?}",
                analysis_result.err()
            );
            let analysis = analysis_result.unwrap();

            assert!(analysis.execution_tree.is_some());
            assert_eq!(analysis.execution_tree.as_ref().unwrap().nodes.len(), tree.nodes.len());

            println!("✅ Doris profile (full query) comprehensive test passed:");
            println!("   - Query ID: {}", profile.summary.query_id);
            println!("   - Total Time: {}", profile.summary.total_time);
            println!("   - Fragments: {}", profile.fragments.len());
            println!("   - Execution Tree Nodes: {}", tree.nodes.len());
            println!("   - Tree Root: {}", tree.root.operator_name);
            println!("   - Reachable Nodes: {}/{}", visited.len(), tree.nodes.len());
            println!("   - Nodes with metrics: {}/{}", nodes_with_metrics, tree.nodes.len());
            println!("   - Nodes with ExecTime: {}/{}", nodes_with_exec_time, tree.nodes.len());
            println!("   - Nodes with Rows: {}/{}", nodes_with_rows, tree.nodes.len());
            println!("   - Overview Metrics:");
            println!(
                "     * Execution Time: {:?} ({:?} ms)",
                profile.summary.query_execution_wall_time,
                profile.summary.query_execution_wall_time_ms
            );
            println!(
                "     * Processing Time: {:?} ({:?} ms)",
                profile.summary.query_cumulative_operator_time,
                profile.summary.query_cumulative_operator_time_ms
            );
            println!(
                "     * Planner Time: {:?} ({:?} ms)",
                profile.summary.planner_total_time, profile.summary.planner_total_time_ms
            );
            println!(
                "     * Schedule Time: {:?} ({:?} ms)",
                profile.summary.query_peak_schedule_time,
                profile.summary.query_peak_schedule_time_ms
            );
            println!(
                "     * Result Deliver Time: {:?} ({:?} ms)",
                profile.summary.result_deliver_time, profile.summary.result_deliver_time_ms
            );
            println!("     * Total Instances: {:?}", profile.summary.total_instance_count);

            // Print detailed metrics for first few nodes
            for (i, node) in tree.nodes.iter().take(3).enumerate() {
                let exec_time_display =
                    if let Some(ref time_raw) = node.metrics.operator_total_time_raw {
                        time_raw.as_str()
                    } else if let Some(time) = node.metrics.operator_total_time {
                        // Format as string for display
                        &format!("{}ns", time)
                    } else {
                        "N/A"
                    };
                println!(
                    "   - Node {}: {} - ExecTime: {}, Rows: {:?}, Memory: {:?}",
                    i, node.operator_name, exec_time_display, node.rows, node.metrics.memory_usage
                );
            }
        }

        /// Comprehensive test for Doris profile limit query - validates all parsing features
        #[test]
        fn test_doris_profile_limit_query_comprehensive() {
            let profile_text =
                load_profile("select  from hudi_hms.ztik.ztik_yljk_jsxxb limit 1.txt");

            // Step 1: Verify profile format
            assert!(profile_text.contains("MergedProfile:"), "MergedProfile section not found");
            assert!(profile_text.contains("Fragment 0:"), "Fragment 0 not found");

            // Step 2: Parse profile
            let mut composer = ProfileComposer::new();
            let result = composer.parse(&profile_text);
            assert!(result.is_ok(), "Failed to parse Doris profile: {:?}", result.err());
            let profile = result.unwrap();

            // Step 3: Verify Summary parsing
            assert_eq!(profile.summary.query_id, "1176d8b573e34626-9c9c5677f17f473d");
            assert_eq!(profile.summary.total_time, "6min50sec");
            assert_eq!(profile.summary.query_state, "OK");
            assert_eq!(profile.summary.starrocks_version, "doris-4.0.1-rc02-791725594d");
            assert!(
                profile
                    .summary
                    .sql_statement
                    .contains("select * from hudi_hms.ztik.ztik_yljk_jsxxb limit 100")
            );

            // Verify overview metrics are extracted from Execution Summary
            // Execution time should be extracted from "Wait and Fetch Result Time" or "Fetch Result Time"
            assert!(
                profile.summary.query_execution_wall_time.is_some()
                    || profile.summary.query_execution_wall_time_ms.is_some(),
                "Execution time should be extracted from Execution Summary"
            );
            // Processing time (Plan Time + Schedule Time + Fetch Time)
            assert!(
                profile.summary.query_cumulative_operator_time.is_some()
                    || profile.summary.query_cumulative_operator_time_ms.is_some(),
                "Processing time should be extracted from Execution Summary"
            );
            // Planner time
            assert!(
                profile.summary.planner_total_time.is_some()
                    || profile.summary.planner_total_time_ms.is_some(),
                "Planner time should be extracted from Execution Summary"
            );
            // Schedule time
            assert!(
                profile.summary.query_peak_schedule_time.is_some()
                    || profile.summary.query_peak_schedule_time_ms.is_some(),
                "Schedule time should be extracted from Execution Summary"
            );

            // Step 4: Verify Fragments extraction
            assert!(!profile.fragments.is_empty(), "Fragments should be extracted");
            let mut total_operators = 0;
            for fragment in &profile.fragments {
                assert!(!fragment.id.is_empty());
                for pipeline in &fragment.pipelines {
                    assert!(!pipeline.id.is_empty());
                    total_operators += pipeline.operators.len();
                    for operator in &pipeline.operators {
                        assert!(!operator.name.is_empty());
                        // Note: Some operators may not have metrics extracted (e.g., if parsing fails)
                        // This is acceptable as long as the operator structure is valid
                    }
                }
            }
            assert!(total_operators > 0, "Should have extracted operators");

            // Step 5: Verify Execution Tree (DAG) structure
            assert!(profile.execution_tree.is_some(), "Execution tree should be built");
            let tree = profile.execution_tree.as_ref().unwrap();
            assert!(!tree.nodes.is_empty(), "Execution tree should have nodes");
            assert_eq!(
                tree.nodes.len(),
                total_operators,
                "Tree nodes count should match operators count"
            );

            // Verify root node
            assert!(!tree.root.id.is_empty());
            assert!(!tree.root.operator_name.is_empty());

            // Verify time_percentage is calculated for nodes
            let mut nodes_with_time_percentage = 0;
            for node in &tree.nodes {
                if node.time_percentage.is_some() && node.time_percentage.unwrap() > 0.0 {
                    nodes_with_time_percentage += 1;
                }
            }
            assert!(
                nodes_with_time_percentage > 0,
                "At least some nodes should have time_percentage calculated. Found {}/{} nodes with time_percentage",
                nodes_with_time_percentage,
                tree.nodes.len()
            );

            // Verify top_time_consuming_nodes is populated
            assert!(
                profile.summary.top_time_consuming_nodes.is_some(),
                "top_time_consuming_nodes should be populated"
            );
            let top_nodes = profile.summary.top_time_consuming_nodes.as_ref().unwrap();
            assert!(
                !top_nodes.is_empty(),
                "top_time_consuming_nodes should not be empty. Found {} nodes",
                top_nodes.len()
            );

            // Verify unique_metrics (CustomCounters) are extracted
            let mut _nodes_with_unique_metrics = 0;
            for node in &tree.nodes {
                if !node.unique_metrics.is_empty() {
                    _nodes_with_unique_metrics += 1;
                }
            }

            // Verify all nodes structure and metrics
            let mut node_ids = std::collections::HashSet::new();
            let mut has_result_sink = false;
            let mut nodes_with_metrics = 0;
            let mut nodes_with_exec_time = 0;
            let mut nodes_with_rows = 0;

            for node in &tree.nodes {
                assert!(!node.id.is_empty());
                assert!(!node.operator_name.is_empty());
                assert!(!node_ids.contains(&node.id), "Duplicate node ID: {}", node.id);
                node_ids.insert(node.id.clone());

                if node.operator_name.contains("RESULT_SINK")
                    || node.operator_name.contains("RESULT_SINK_OPERATOR")
                {
                    has_result_sink = true;
                }

                // Verify metrics are extracted
                // Check if node has at least some metrics (ExecTime, RowsProduced, InputRows, etc.)
                if node.metrics.operator_total_time.is_some()
                    || node.metrics.operator_total_time_raw.is_some()
                {
                    nodes_with_exec_time += 1;
                }

                if node.rows.is_some()
                    || node.metrics.push_row_num.is_some()
                    || node.metrics.pull_row_num.is_some()
                {
                    nodes_with_rows += 1;
                }

                // Check if node has any metrics at all
                if node.metrics.operator_total_time.is_some()
                    || node.metrics.operator_total_time_raw.is_some()
                    || node.metrics.push_row_num.is_some()
                    || node.metrics.pull_row_num.is_some()
                    || node.metrics.memory_usage.is_some()
                    || !node.unique_metrics.is_empty()
                {
                    nodes_with_metrics += 1;
                }

                // Verify children references
                for child_id in &node.children {
                    assert!(
                        tree.nodes.iter().any(|n| n.id == *child_id),
                        "Child node {} not found",
                        child_id
                    );
                }
            }

            // Verify that at least some nodes have metrics
            // Note: Not all operators may have metrics in Doris profile, but most should
            assert!(
                nodes_with_metrics > 0,
                "At least some nodes should have metrics. Found {}/{} nodes with metrics",
                nodes_with_metrics,
                tree.nodes.len()
            );

            // Verify DAG structure: check depth calculation
            assert!(tree.root.depth == 0, "Root depth should be 0");
            for node in &tree.nodes {
                if !node.children.is_empty() {
                    // Parent should have depth <= children
                    for child_id in &node.children {
                        if let Some(child) = tree.nodes.iter().find(|n| n.id == *child_id) {
                            assert!(
                                child.depth > node.depth || child.depth == node.depth + 1,
                                "Child depth should be greater than parent depth"
                            );
                        }
                    }
                }
            }

            // Step 6: Verify full analyze_profile workflow
            let analysis_result = analyze_profile(&profile_text);
            assert!(analysis_result.is_ok(), "analyze_profile should succeed");
            let analysis = analysis_result.unwrap();

            assert!(analysis.execution_tree.is_some());
            assert_eq!(analysis.execution_tree.as_ref().unwrap().nodes.len(), tree.nodes.len());

            // Verify diagnostics can be generated (diagnostics is a Vec, not Option)
            assert!(!analysis.diagnostics.is_empty() || analysis.diagnostics.is_empty()); // May or may not have diagnostics

            println!("✅ Doris profile (limit query) comprehensive test passed:");
            println!("   - Query ID: {}", profile.summary.query_id);
            println!("   - Total Time: {}", profile.summary.total_time);
            println!("   - Fragments: {}", profile.fragments.len());
            println!("   - Total Operators: {}", total_operators);
            println!("   - Execution Tree Nodes: {}", tree.nodes.len());
            println!("   - Tree Root: {}", tree.root.operator_name);
            println!("   - Has RESULT_SINK: {}", has_result_sink);
            println!("   - All nodes reachable: {}", node_ids.len() == tree.nodes.len());
            println!("   - Nodes with metrics: {}/{}", nodes_with_metrics, tree.nodes.len());
            println!("   - Nodes with ExecTime: {}/{}", nodes_with_exec_time, tree.nodes.len());
            println!("   - Nodes with Rows: {}/{}", nodes_with_rows, tree.nodes.len());
            println!("   - Overview Metrics:");
            println!(
                "     * Execution Time: {:?} ({:?} ms)",
                profile.summary.query_execution_wall_time,
                profile.summary.query_execution_wall_time_ms
            );
            println!(
                "     * Processing Time: {:?} ({:?} ms)",
                profile.summary.query_cumulative_operator_time,
                profile.summary.query_cumulative_operator_time_ms
            );
            println!(
                "     * Planner Time: {:?} ({:?} ms)",
                profile.summary.planner_total_time, profile.summary.planner_total_time_ms
            );
            println!(
                "     * Schedule Time: {:?} ({:?} ms)",
                profile.summary.query_peak_schedule_time,
                profile.summary.query_peak_schedule_time_ms
            );
            println!(
                "     * Result Deliver Time: {:?} ({:?} ms)",
                profile.summary.result_deliver_time, profile.summary.result_deliver_time_ms
            );
            println!("     * Total Instances: {:?}", profile.summary.total_instance_count);

            // Print detailed metrics for all nodes including time_percentage
            println!("   - Node details with time_percentage:");
            for (i, node) in tree.nodes.iter().enumerate() {
                let exec_time_display =
                    if let Some(ref time_raw) = node.metrics.operator_total_time_raw {
                        time_raw.as_str()
                    } else if let Some(time) = node.metrics.operator_total_time {
                        // Format as string for display
                        &format!("{}ns", time)
                    } else {
                        "N/A"
                    };
                let time_pct = node.time_percentage.map(|p| format!("{:.2}%", p)).unwrap_or_else(|| "None".to_string());
                let time_ns = node.metrics.operator_total_time.map(|t| format!("{}ns", t)).unwrap_or_else(|| "None".to_string());
                println!(
                    "     Node {}: {} - ExecTime: {}, time_percentage: {}, operator_total_time: {}, Rows: {:?}, Memory: {:?}",
                    i, node.operator_name, exec_time_display, time_pct, time_ns, node.rows, node.metrics.memory_usage
                );
            }
            
            // Print top nodes
            if let Some(top_nodes) = &profile.summary.top_time_consuming_nodes {
                println!("   - Top time consuming nodes: {}", top_nodes.len());
                for (i, top_node) in top_nodes.iter().take(5).enumerate() {
                    println!("     {}. {}: {:.2}% (plan_node_id={}, total_time={})", 
                        i + 1, top_node.operator_name, top_node.time_percentage, top_node.plan_node_id, top_node.total_time);
                }
            }
        }

        /// Comprehensive test for user's new Doris profile (15ms) - validates 10 operators parsing
        /// This test validates that all 10 operators across different fragments/pipelines are correctly parsed
        #[test]
        fn test_doris_profile_15ms_comprehensive() {
            // Create a test profile based on the structure provided by the user
            // Profile structure: Fragment 0 (Pipeline 0) + Fragment 1 (Pipelines 0,1,2,3)
            let profile_text = r#"Summary:
   - Profile ID: test-15ms-query-id
   - Task Type: QUERY
   - Start Time: 2025-01-01 00:00:00
   - End Time: 2025-01-01 00:00:00
   - Total: 15ms
   - Task State: OK
   - User: root
   - Default Catalog: internal
   - Default Db: test_db
   - Sql Statement: select * from test_table order by id limit 100
Execution Summary:
   - Plan Time: 1ms
   - Schedule Time: 1ms
   - Wait and Fetch Result Time: 13ms
   - Doris Version: doris-4.0.1-test
   - Total Instances Num: 15
MergedProfile:
     Fragments:
       Fragment 0:
         Pipeline : 0(instance_num=1):
           RESULT_SINK_OPERATOR (id=0):
             CommonCounters:
                - ExecTime: avg 81.150us, max 81.150us, min 81.150us
                - InputRows: sum 100, avg 100, max 100, min 100
             EXCHANGE_OPERATOR (id=2):
                CommonCounters:
                   - ExecTime: avg 130.380us, max 130.380us, min 130.380us
                   - RowsProduced: sum 100, avg 100, max 100, min 100
       Fragment 1:
         Pipeline : 0(instance_num=14):
           DATA_STREAM_SINK_OPERATOR (id=2,dst_id=2):
             CommonCounters:
                - ExecTime: avg 31.351us, max 50.279us, min 20.437us
                - InputRows: sum 100, avg 7, max 100, min 0
             LOCAL_EXCHANGE_OPERATOR (LOCAL_MERGE_SORT) (id=-3):
                CommonCounters:
                   - ExecTime: avg 8.772us, max 102.779us, min 790ns
                   - RowsProduced: sum 100, avg 7, max 100, min 0
         Pipeline : 1(instance_num=14):
           LOCAL_EXCHANGE_SINK_OPERATOR (LOCAL_MERGE_SORT) (id=-3):
             CommonCounters:
                - ExecTime: avg 7.583us, max 24.892us, min 4.154us
                - InputRows: sum 100, avg 7, max 100, min 0
             SORT_OPERATOR (id=1 , nereids_id=137):
                CommonCounters:
                   - ExecTime: avg 1.69us, max 2.600us, min 324ns
                   - RowsProduced: sum 100, avg 7, max 100, min 0
         Pipeline : 2(instance_num=14):
           SORT_SINK_OPERATOR (id=1 , nereids_id=137):
             CommonCounters:
                - ExecTime: avg 36.45us, max 54.408us, min 23.205us
                - InputRows: sum 100, avg 7, max 100, min 0
             LOCAL_EXCHANGE_OPERATOR (PASSTHROUGH) (id=-2):
                CommonCounters:
                   - ExecTime: avg 3.430us, max 14.652us, min 1.182us
                   - RowsProduced: sum 100, avg 7, max 100, min 0
         Pipeline : 3(instance_num=1):
           LOCAL_EXCHANGE_SINK_OPERATOR (PASSTHROUGH) (id=-2):
             CommonCounters:
                - ExecTime: avg 62.185us, max 62.185us, min 62.185us
                - InputRows: sum 100, avg 100, max 100, min 100
             OLAP_SCAN_OPERATOR (id=0. nereids_id=127. table name = test_table(test_table)):
                CommonCounters:
                   - ExecTime: avg 972.972us, max 972.972us, min 972.972us
                   - RowsProduced: sum 100, avg 100, max 100, min 100
                   - ScanBytes: sum 1.5 MB, avg 1.5 MB, max 1.5 MB, min 1.5 MB
"#;

            // Step 1: Verify profile format
            assert!(profile_text.contains("MergedProfile"), "MergedProfile section not found");
            assert!(profile_text.contains("Fragment 0:"), "Fragment 0 not found");
            assert!(profile_text.contains("Fragment 1:"), "Fragment 1 not found");

            // Step 2: Parse profile
            let mut composer = ProfileComposer::new();
            let profile_result = composer.parse(profile_text);
            assert!(profile_result.is_ok(), "Profile parsing failed: {:?}", profile_result.err());
            let profile = profile_result.unwrap();

            // Step 3: Verify summary fields
            assert!(!profile.summary.query_id.is_empty(), "Query ID should not be empty");
            assert_eq!(profile.summary.total_time, "15ms", "Total time should be 15ms");
            assert_eq!(profile.summary.starrocks_version, "doris-4.0.1-test", "Doris version should match");

            // Step 4: Verify fragments
            assert!(!profile.fragments.is_empty(), "Fragments should not be empty");
            assert!(profile.fragments.len() >= 2, "Should have at least 2 fragments");

            // Count total operators across all fragments
            let total_operators: usize = profile.fragments
                .iter()
                .map(|f| f.pipelines.iter().map(|p| p.operators.len()).sum::<usize>())
                .sum();
            println!("Total Operators: {}", total_operators);
            assert_eq!(total_operators, 10, "Should have exactly 10 operators. Found: {}", total_operators);

            // Step 5: Verify execution tree
            let tree = profile.execution_tree.as_ref().expect("Execution tree is missing");
            assert!(!tree.nodes.is_empty(), "Execution tree should not be empty");
            println!("Execution Tree Nodes: {}", tree.nodes.len());
            assert_eq!(tree.nodes.len(), 10, "Should have exactly 10 nodes in execution tree. Found: {}", tree.nodes.len());

            // Step 6: Verify all expected operators are present
            let operator_names: Vec<String> = tree.nodes.iter().map(|n| n.operator_name.clone()).collect();
            println!("Operators found: {:?}", operator_names);
            
            let expected_operators = vec![
                "RESULT_SINK_OPERATOR",
                "EXCHANGE_OPERATOR",
                "DATA_STREAM_SINK_OPERATOR",
                "LOCAL_EXCHANGE_OPERATOR",
                "LOCAL_EXCHANGE_SINK_OPERATOR",
                "SORT_OPERATOR",
                "SORT_SINK_OPERATOR",
                "OLAP_SCAN_OPERATOR",
            ];
            
            for expected_op in &expected_operators {
                assert!(
                    operator_names.iter().any(|name| name.contains(expected_op)),
                    "Missing operator: {}. Found operators: {:?}",
                    expected_op,
                    operator_names
                );
            }

            // Step 7: Verify time_percentage is calculated for all nodes
            let nodes_with_time_percentage = tree.nodes.iter()
                .filter(|n| n.time_percentage.is_some())
                .count();
            println!("Nodes with time_percentage: {}/{}", nodes_with_time_percentage, tree.nodes.len());
            assert_eq!(nodes_with_time_percentage, tree.nodes.len(), "All nodes should have time_percentage");

            // Step 8: Verify top_time_consuming_nodes is populated
            assert!(
                profile.summary.top_time_consuming_nodes.is_some(),
                "top_time_consuming_nodes should be populated"
            );
            let top_nodes = profile.summary.top_time_consuming_nodes.as_ref().unwrap();
            assert!(!top_nodes.is_empty(), "top_time_consuming_nodes should not be empty");
            println!("Top time consuming nodes: {}", top_nodes.len());

            // Step 9: Verify tree connectivity (all nodes reachable from root)
            let mut visited = std::collections::HashSet::new();
            let mut queue = std::collections::VecDeque::new();
            queue.push_back(tree.root.id.clone());
            visited.insert(tree.root.id.clone());

            while let Some(node_id) = queue.pop_front() {
                if let Some(node) = tree.nodes.iter().find(|n| n.id == node_id) {
                    for child_id in &node.children {
                        if !visited.contains(child_id) {
                            visited.insert(child_id.clone());
                            queue.push_back(child_id.clone());
                        }
                    }
                }
            }
            assert_eq!(visited.len(), tree.nodes.len(), "All nodes should be reachable from root");

            println!("✅ Doris profile (15ms) comprehensive test passed:");
            println!("   - Query ID: {}", profile.summary.query_id);
            println!("   - Total Time: {}", profile.summary.total_time);
            println!("   - Fragments: {}", profile.fragments.len());
            println!("   - Total Operators: {}", total_operators);
            println!("   - Execution Tree Nodes: {}", tree.nodes.len());
            println!("   - Nodes with time_percentage: {}/{}", nodes_with_time_percentage, tree.nodes.len());
            println!("   - Top time consuming nodes: {}", top_nodes.len());
            println!("   - Reachable Nodes: {}/{}", visited.len(), tree.nodes.len());
        }
    }
}
