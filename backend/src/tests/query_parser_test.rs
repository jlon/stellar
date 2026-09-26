use crate::handlers::query::{is_materialized_view_write, parse_sql_statements};

#[test]
fn preserves_semicolons_inside_literals_and_identifiers() {
    let statements = parse_sql_statements(
        r#"LOAD LABEL `analytics`.`label;daily` (DATA INFILE ("hdfs://namenode:8020/data;daily.csv") INTO TABLE `events;daily`) WITH BROKER; SELECT "complete;value";"#,
    );

    assert_eq!(statements.len(), 2);
    assert!(statements[0].contains("label;daily"));
    assert!(statements[0].contains("data;daily.csv"));
    assert_eq!(statements[1], r#"SELECT "complete;value""#);
}

#[test]
fn preserves_semicolons_after_backslash_escaped_quotes() {
    let statements = parse_sql_statements(r#"SELECT "value with \"; still quoted"; SELECT 1"#);

    assert_eq!(statements, vec![r#"SELECT "value with \"; still quoted""#, "SELECT 1"]);
}

#[test]
fn identifies_materialized_view_writes_for_the_dedicated_api() {
    for statement in [
        "CREATE MATERIALIZED VIEW sales_mv AS SELECT 1",
        "CREATE /* controlled */ MATERIALIZED\nVIEW sales_mv AS SELECT 1",
        "/* maintenance */ DROP MATERIALIZED VIEW sales_mv",
        "ALTER MATERIALIZED VIEW sales_mv RENAME next_sales_mv",
        "REFRESH MATERIALIZED VIEW sales_mv",
        "CANCEL REFRESH MATERIALIZED VIEW sales_mv",
        "PAUSE MATERIALIZED VIEW JOB ON sales_mv",
        "RESUME MATERIALIZED VIEW JOB ON sales_mv",
        "ALTER TABLE `analytics`.`sales` ADD ROLLUP sales_mv (day)",
        "ALTER TABLE analytics.sales DROP ROLLUP sales_mv",
    ] {
        assert!(is_materialized_view_write(statement), "{statement}");
    }

    for statement in [
        "SHOW CREATE MATERIALIZED VIEW sales_mv",
        "SELECT 'CREATE MATERIALIZED VIEW sales_mv'",
        "EXPLAIN SELECT * FROM sales",
    ] {
        assert!(!is_materialized_view_write(statement), "{statement}");
    }

    let batch = parse_sql_statements("SELECT 1; CREATE MATERIALIZED VIEW sales_mv AS SELECT 1");
    assert!(
        batch
            .iter()
            .any(|statement| is_materialized_view_write(statement))
    );
}
