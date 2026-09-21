use crate::handlers::query::parse_sql_statements;

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
