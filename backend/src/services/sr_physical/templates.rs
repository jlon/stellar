use std::collections::HashSet;

const BEGIN: &str = "# BEGIN STELLAR MANAGED CONFIG";
const END: &str = "# END STELLAR MANAGED CONFIG";

pub fn merge_managed_config(existing: &str, values: &[(&str, String)]) -> String {
    let managed_keys: HashSet<&str> = values.iter().map(|(key, _)| *key).collect();
    let mut output: Vec<String> = Vec::new();
    let mut in_managed_block = false;

    for line in existing.lines() {
        if line.trim() == BEGIN {
            in_managed_block = true;
            continue;
        }
        if line.trim() == END {
            in_managed_block = false;
            continue;
        }
        if in_managed_block || is_managed_key(line, &managed_keys) {
            continue;
        }
        output.push(line.to_owned());
    }

    while output.last().is_some_and(|line| line.trim().is_empty()) {
        output.pop();
    }
    if !output.is_empty() {
        output.push(String::new());
    }
    output.push(BEGIN.to_owned());
    for (key, value) in values {
        output.push(format!("{key} = {value}"));
    }
    output.push(END.to_owned());
    output.join("\n") + "\n"
}

pub struct FeConfig<'a> {
    pub advertise_host: &'a str,
    pub meta_dir: &'a str,
    pub edit_log_port: i64,
    pub http_port: i64,
    pub query_port: i64,
    pub rpc_port: i64,
    pub single_be: bool,
}

/// Managed keys are topology facts controlled by the deployment model; a
/// configuration change must not alter them, only add or tune other lines.
pub const FE_MANAGED_KEYS: [&str; 6] =
    ["meta_dir", "priority_networks", "edit_log_port", "http_port", "query_port", "rpc_port"];
pub const BE_MANAGED_KEYS: [&str; 7] = [
    "storage_root_path",
    "priority_networks",
    "heartbeat_service_port",
    "be_port",
    "be_http_port",
    "brpc_port",
    "starlet_port",
];

pub fn fe_config(existing: &str, input: FeConfig<'_>) -> String {
    let mut values = vec![
        ("meta_dir", input.meta_dir.to_owned()),
        ("priority_networks", format!("{}/32", input.advertise_host)),
        ("edit_log_port", input.edit_log_port.to_string()),
        ("http_port", input.http_port.to_string()),
        ("query_port", input.query_port.to_string()),
        ("rpc_port", input.rpc_port.to_string()),
    ];
    if input.single_be {
        values.push(("default_replication_num", "1".to_string()));
    }
    merge_managed_config(existing, &values)
}

pub fn be_config(
    existing: &str,
    advertise_host: &str,
    storage_dir: &str,
    heartbeat_port: i64,
    be_port: i64,
    webserver_port: i64,
    brpc_port: i64,
    starlet_port: i64,
) -> String {
    merge_managed_config(
        existing,
        &[
            ("storage_root_path", storage_dir.to_owned()),
            ("priority_networks", format!("{advertise_host}/32")),
            ("heartbeat_service_port", heartbeat_port.to_string()),
            ("be_port", be_port.to_string()),
            ("be_http_port", webserver_port.to_string()),
            ("brpc_port", brpc_port.to_string()),
            ("starlet_port", starlet_port.to_string()),
        ],
    )
}

fn is_managed_key(line: &str, managed_keys: &HashSet<&str>) -> bool {
    let Some((key, _)) = line.split_once('=') else {
        return false;
    };
    managed_keys.contains(key.trim())
}

/// Extracts `key = value` pairs (comments and blank lines ignored) for the
/// managed-key comparison performed before any configuration write.
pub fn extract_config_values(content: &str) -> Vec<(String, String)> {
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let line = line.split('#').next()?.trim();
            if line.is_empty() {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::merge_managed_config;

    #[test]
    fn replaces_only_managed_keys_and_is_idempotent() {
        let once = merge_managed_config(
            "JAVA_OPTS = -Xmx1g\nhttp_port = 9999\n",
            &[("http_port", "8030".to_string())],
        );
        let twice = merge_managed_config(&once, &[("http_port", "8030".to_string())]);

        assert!(once.contains("JAVA_OPTS = -Xmx1g"));
        assert!(once.contains("http_port = 8030"));
        assert_eq!(once, twice);
    }
}
