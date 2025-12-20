/// Permission extraction module for cleaner code organization
/// Uses strategy pattern to handle different route patterns
/// Extract permission from URI and method
pub fn extract_permission(method: &str, uri: &str) -> Option<(String, String)> {
    if uri == "/api/auth/permissions" {
        return None;
    }

    if uri == "/api/clusters/active" && method == "GET" {
        return None;
    }

    let path = uri.strip_prefix("/api/").unwrap_or(uri);
    let segments: Vec<&str> = path.split('/').collect();

    // Special handling for /api/clusters/db-auth/* paths
    // db-auth is a separate resource in the permissions model, not under clusters
    if segments.get(0) == Some(&"clusters") && segments.get(1) == Some(&"db-auth") {
        if method == "GET" && segments.len() == 3 {
            let action = match segments.get(2) {
                Some(&"accounts") => Some("accounts:list".to_string()),
                Some(&"roles") => Some("roles:list".to_string()),
                _ => None,
            }?;
            return Some(("db-auth".to_string(), action));
        }
        return None;
    }

    let resource = match *(segments.first()?) {
        "roles" => "roles",
        "permissions" => "permissions",
        "users" => "users",
        "clusters" => "clusters",
        _ => return None,
    };

    let action = extract_action_with_special_handlers(resource, &segments, method)
        .or_else(|| extract_action_default(resource, &segments, method))?;

    Some((resource.to_string(), action))
}

/// Extract action with special route handlers
fn extract_action_with_special_handlers(
    resource: &str,
    segments: &[&str],
    method: &str,
) -> Option<String> {
    match resource {
        "roles" => extract_roles_action(segments, method),
        "users" => extract_users_action(segments, method),
        "clusters" => extract_clusters_action_special(segments, method),
        _ => None,
    }
}

/// Extract action for roles resource
fn extract_roles_action(segments: &[&str], method: &str) -> Option<String> {
    if segments.len() >= 3 && segments.get(2) == Some(&"permissions") {
        return match method {
            "PUT" => Some("permissions:update".to_string()),
            "GET" => Some("permissions:get".to_string()),
            _ => None,
        };
    }
    None
}

/// Extract action for users resource
fn extract_users_action(segments: &[&str], method: &str) -> Option<String> {
    if segments.len() >= 3 && segments.get(2) == Some(&"roles") {
        return match method {
            "POST" => Some("roles:assign".to_string()),
            "DELETE" => Some("roles:remove".to_string()),
            "GET" => Some("roles:get".to_string()),
            _ => None,
        };
    }
    None
}

/// Extract action for clusters resource with special handlers
fn extract_clusters_action_special(segments: &[&str], method: &str) -> Option<String> {
    if segments.len() < 2 {
        return None;
    }

    let second = *segments.get(1)?;

    if second.parse::<i64>().is_ok() {
        return extract_clusters_id_action(segments, method);
    }

    extract_clusters_special_paths(segments, method)
}

/// Extract action for clusters/{id} paths
fn extract_clusters_id_action(segments: &[&str], method: &str) -> Option<String> {
    match segments.len() {
        2 => match method {
            "GET" => Some("get".to_string()),
            "PUT" => Some("update".to_string()),
            "DELETE" => Some("delete".to_string()),
            _ => None,
        },
        _ if segments.len() >= 3 => {
            let action = segments.get(2)?;

            // Special handling for db-auth routes
            if *action == "db-auth" && segments.len() >= 4 {
                let db_action = segments.get(3)?;
                return match (*db_action, method) {
                    ("accounts", "GET") => Some("db-auth:accounts:list".to_string()),
                    ("roles", "GET") => Some("db-auth:roles:list".to_string()),
                    _ => Some("db-auth".to_string()),
                };
            }

            if method == "POST" && *action == "health" {
                Some("health:post".to_string())
            } else if method == "POST" && *action == "sql" && segments.get(3) == Some(&"diagnose") {
                Some("sql:diagnose".to_string())
            } else {
                Some(action.to_string())
            }
        },
        _ => None,
    }
}

/// Extract action for special cluster paths
type RouteHandler = Box<dyn Fn(&[&str], &str) -> Option<String>>;

fn extract_clusters_special_paths(segments: &[&str], method: &str) -> Option<String> {
    let _second = segments.get(1)?;
    let _len = segments.len();

    let handlers: Vec<RouteHandler> = vec![
        // Handle /api/clusters/db-auth/accounts and /api/clusters/db-auth/roles
        // Note: db-auth is a separate resource in permissions, not a clusters sub-action
        // So we need to handle it specially to extract it as a separate resource
        Box::new(|seg, m| {
            if m == "GET" && seg.len() == 3 && seg.get(1) == Some(&"db-auth") {
                // This handler returns None here because db-auth needs to be extracted
                // at resource level, not action level. The caller should detect this pattern.
                None
            } else {
                None
            }
        }),
        Box::new(|seg, m| {
            if m == "DELETE" && seg.len() == 4 && seg.get(1) == Some(&"backends") {
                Some("backends:delete".to_string())
            } else {
                None
            }
        }),
        Box::new(|seg, m| {
            if m == "DELETE" && seg.len() >= 3 && seg.get(1) == Some(&"queries") {
                if let Some(second) = seg.get(2)
                    && (*second == "history" || *second == "execute")
                {
                    return None;
                }

                Some("queries:kill".to_string())
            } else {
                None
            }
        }),
        Box::new(|seg, m| {
            if m == "GET" && seg.len() >= 4 && seg.get(1) == Some(&"queries") {
                if let Some(last) = seg.last()
                    && *last == "profile"
                {
                    if let Some(second) = seg.get(2)
                        && (*second == "history" || *second == "execute")
                    {
                        return None;
                    }
                    return Some("queries:profile".to_string());
                }
                None
            } else {
                None
            }
        }),
        Box::new(|seg, m| {
            if m == "GET" && seg.len() >= 3 && seg.get(1) == Some(&"profiles") {
                Some("profiles:get".to_string())
            } else {
                None
            }
        }),
        Box::new(|seg, m| {
            if m == "DELETE" && seg.len() >= 3 && seg.get(1) == Some(&"sessions") {
                Some("sessions:kill".to_string())
            } else {
                None
            }
        }),
        Box::new(extract_materialized_views_action),
        Box::new(extract_variables_action),
        Box::new(extract_system_functions_action),
        Box::new(extract_sql_blacklist_action),
    ];

    for handler in handlers {
        if let Some(action) = handler(segments, method) {
            return Some(action);
        }
    }

    None
}

/// Extract action for materialized_views paths
fn extract_materialized_views_action(segments: &[&str], method: &str) -> Option<String> {
    if segments.get(1) != Some(&"materialized_views") || segments.len() < 3 {
        return None;
    }

    match segments.len() {
        3 => match method {
            "GET" => Some("materialized_views:get".to_string()),
            "PUT" => Some("materialized_views:update".to_string()),
            "DELETE" => Some("materialized_views:delete".to_string()),
            _ => None,
        },
        4 => {
            let action = segments.get(3)?;
            match (*action, method) {
                ("ddl", "GET") => Some("materialized_views:ddl".to_string()),
                ("refresh", "POST") => Some("materialized_views:refresh".to_string()),
                ("cancel", "POST") => Some("materialized_views:cancel".to_string()),
                _ => None,
            }
        },
        _ => None,
    }
}

/// Extract action for variables paths
fn extract_variables_action(segments: &[&str], method: &str) -> Option<String> {
    if method == "PUT" && segments.len() == 3 && segments.get(1) == Some(&"variables") {
        segments.get(2).and_then(|third| {
            if third.parse::<i64>().is_err() { Some("variables:update".to_string()) } else { None }
        })
    } else {
        None
    }
}

/// Extract action for system-functions paths
fn extract_system_functions_action(segments: &[&str], method: &str) -> Option<String> {
    if segments.get(1) != Some(&"system-functions") || segments.len() < 3 {
        return None;
    }

    let third = segments.get(2)?;
    if third.parse::<i64>().is_err() {
        return None;
    }

    match segments.len() {
        3 => match method {
            "PUT" => Some("system:functions:update".to_string()),
            "DELETE" => Some("system:functions:delete".to_string()),
            _ => None,
        },
        4 => {
            let fourth = segments.get(3)?;
            match (*fourth, method) {
                ("execute", "POST") => Some("system:functions:execute".to_string()),
                ("favorite", "PUT") => Some("system:functions:favorite".to_string()),
                _ => None,
            }
        },
        _ => None,
    }
}

/// Extract action for sql-blacklist paths
fn extract_sql_blacklist_action(segments: &[&str], method: &str) -> Option<String> {
    if segments.len() >= 2 && segments.get(1) == Some(&"sql-blacklist") {
        match segments.len() {
            2 => match method {
                "GET" => Some("sql:blacklist".to_string()),
                "POST" => Some("sql:blacklist:add".to_string()),
                _ => None,
            },
            3 => match method {
                "DELETE" => Some("sql:blacklist:delete".to_string()),
                _ => None,
            },
            _ => None,
        }
    } else {
        None
    }
}

/// Default action extraction for general cases
/// This handles clusters non-ID paths and other generic routes
fn extract_action_default(resource: &str, segments: &[&str], method: &str) -> Option<String> {
    if resource == "clusters" && segments.len() >= 2 {
        let action_parts: Vec<&str> = segments.iter().skip(1).copied().collect();
        let action_str = action_parts.join(":").replace("-", ":");
        return Some(action_str);
    }

    if segments.len() >= 2 {
        let second = segments.get(1).copied();

        if let Some(second_str) = second
            && second_str.parse::<i64>().is_ok()
        {
            return match method {
                "GET" => Some("get".to_string()),
                "PUT" => Some("update".to_string()),
                "DELETE" => Some("delete".to_string()),
                _ => None,
            };
        }

        match method {
            "GET" => second.map(|s| s.to_string()),
            _ => None,
        }
    } else {
        match method {
            "GET" => Some("list".to_string()),
            "POST" => Some("create".to_string()),
            _ => None,
        }
    }
}
