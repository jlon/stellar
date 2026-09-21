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

    // System support archive: /api/system/logs/archive
    if segments.as_slice() == ["system", "logs", "archive"] && method == "GET" {
        return Some(("system".to_string(), "logs:archive".to_string()));
    }

    // Op audit logs: /api/op-audit-logs -- 平台操作审计
    if segments.first() == Some(&"op-audit-logs") {
        return Some(("op-audit-logs".to_string(), "logs:list".to_string()));
    }

    // Notifications resource: /api/notifications -- 用户通知（铃铛）
    if segments.first() == Some(&"notifications") {
        return Some((
            "notifications".to_string(),
            extract_notifications_action(&segments, method)?,
        ));
    }

    // Chat actions resource: /api/agent/chat-actions -- 对话内动作确认
    if segments.first() == Some(&"agent") && segments.get(1) == Some(&"chat-actions") {
        return match (segments.len(), method, segments.get(2).copied()) {
            // 资源复用 "agent"（权限码 api:agent:chat-actions:*，与播种一致）
            (2, "GET", _) => Some(("agent".to_string(), "chat-actions:list".to_string())),
            (3, "POST", Some("confirm")) => {
                Some(("agent".to_string(), "chat-actions:confirm".to_string()))
            },
            (3, "POST", Some("cancel")) => {
                Some(("agent".to_string(), "chat-actions:cancel".to_string()))
            },
            _ => None,
        };
    }

    // Agent resource: /api/agent/* -- AI 运维助手权限
    if segments.first() == Some(&"agent") {
        return Some(("agent".to_string(), extract_agent_action(&segments, method)?));
    }

    // Permission requests are a workflow resource, not an implicit clusters fallback.
    if segments.first() == Some(&"permission-requests") {
        let action =
            match (segments.len(), method, segments.get(1).copied(), segments.get(2).copied()) {
                (2, "GET", Some("my"), _) => "my",
                (2, "GET", Some("pending"), _) => "pending",
                (1, "POST", _, _) => "create",
                (2, "GET", _, _) => "get",
                (3, "POST", _, Some("approve")) => "approve",
                (3, "POST", _, Some("reject")) => "reject",
                (3, "POST", _, Some("cancel")) => "cancel",
                _ => return None,
            };
        return Some(("permission-requests".to_string(), action.to_string()));
    }

    if segments.first() == Some(&"db-auth")
        && segments.get(1) == Some(&"preview-sql")
        && method == "POST"
    {
        return Some(("db-auth".to_string(), "preview-sql".to_string()));
    }

    // Special handling for /api/clusters/db-auth/* paths
    // db-auth is a separate resource in the permissions model, not under clusters
    if segments.first() == Some(&"clusters") && segments.get(1) == Some(&"db-auth") {
        if method == "GET" && segments.len() == 3 {
            let action = match segments.get(2) {
                Some(&"accounts") => Some("accounts:list".to_string()),
                Some(&"roles") => Some("roles:list".to_string()),
                Some(&"my-permissions") => Some("my-permissions".to_string()),
                _ => None,
            }?;
            return Some(("db-auth".to_string(), action));
        }
        if method == "GET" && segments.len() == 4 && segments.get(2) == Some(&"role-permissions") {
            return Some(("db-auth".to_string(), "role-permissions".to_string()));
        }
        return None;
    }

    if segments.first() == Some(&"clusters")
        && segments.get(2) == Some(&"db-auth")
        && method == "GET"
        && segments.len() == 4
    {
        let action = match segments.get(3) {
            Some(&"accounts") => "accounts:list",
            Some(&"roles") => "roles:list",
            _ => return None,
        };
        return Some(("db-auth".to_string(), action.to_string()));
    }

    // Special handling for /api/clusters/resource-groups/* paths
    // resource-groups is a separate resource in the permissions model,
    // matching the seeded api:resource-groups:* permission codes
    if segments.first() == Some(&"clusters") && segments.get(1) == Some(&"resource-groups") {
        let action = extract_resource_groups_action(&segments, method)?;
        return Some(("resource-groups".to_string(), action));
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

/// Extract action for notifications resource (/api/notifications/*)
fn extract_notifications_action(segments: &[&str], method: &str) -> Option<String> {
    match (segments.len(), method, segments.get(2).copied()) {
        (2, "GET", _) => Some("list".to_string()),
        (2, "POST", _) => Some("create".to_string()),
        (3, "POST", Some("read")) => Some("read".to_string()),
        _ => None,
    }
}

/// Extract action for agent resource (/api/agent/*)
fn extract_agent_action(segments: &[&str], method: &str) -> Option<String> {
    match (segments.get(1).copied(), segments.len(), method) {
        (Some("chat"), 2, "POST") => Some("chat".to_string()),
        (Some("chat"), 3, "POST") if segments.get(2) == Some(&"stream") => Some("chat".to_string()),
        (Some("sessions"), 2, "GET") => Some("sessions".to_string()),
        (Some("sessions"), 3, "GET") => Some("sessions:get".to_string()),
        (Some("sessions"), 3, "PATCH") => Some("sessions:rename".to_string()),
        (Some("sessions"), 3, "DELETE") => Some("sessions:delete".to_string()),
        // 事件闭环（runtime）
        (Some("incidents"), 2, "GET") => Some("incidents".to_string()),
        (Some("incidents"), 3, "GET") if segments.get(2) == Some(&"actions") => {
            Some("incidents:actions:get".to_string())
        },
        (Some("incidents"), 3, "GET") => Some("incidents:get".to_string()),
        (Some("incidents"), 4, "POST") => match segments.get(3).copied() {
            Some("investigate") => Some("incidents:investigate".to_string()),
            Some("analyze") => Some("incidents:analyze".to_string()),
            Some("close") => Some("incidents:close".to_string()),
            Some("actions") => Some("incidents:actions".to_string()),
            _ => None,
        },
        (Some("incidents"), 5, "POST") if segments.get(3) == Some(&"actions") => {
            match segments.get(4).copied() {
                Some("confirm") => Some("incidents:actions:confirm".to_string()),
                Some("cancel") => Some("incidents:actions:cancel".to_string()),
                _ => None,
            }
        },
        (Some("events"), 2, "GET") => Some("events".to_string()),
        // 回答点赞/点踩：POST /api/agent/messages/:id/feedback
        (Some("messages"), 4, "POST") if segments.get(3) == Some(&"feedback") => {
            Some("messages:feedback".to_string())
        },
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
        // 导入任务使用独立权限，与菜单和路由守卫保持一致。
        Box::new(|seg, m| {
            if m == "GET" && seg.get(1) == Some(&"loads") {
                Some("loads".to_string())
            } else {
                None
            }
        }),
        // 处置动作会向引擎下发语句，沿用 SQL 执行权限。
        Box::new(|seg, m| {
            if m == "POST" && seg.get(1) == Some(&"loads") {
                Some("queries:execute".to_string())
            } else {
                None
            }
        }),
        Box::new(|seg, m| {
            if m == "POST" && seg == ["clusters", "queries", "stream-load"] {
                Some("queries:execute".to_string())
            } else {
                None
            }
        }),
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
            // 按指纹取消在跑查询：与 KILL 同风险等级，复用 queries:kill 权限码（零迁移）。
            if m == "POST"
                && seg.len() == 3
                && seg.get(1) == Some(&"queries")
                && seg.get(2) == Some(&"cancel")
            {
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
        Box::new(extract_resource_groups_action),
    ];

    for handler in handlers {
        if let Some(action) = handler(segments, method) {
            return Some(action);
        }
    }

    None
}

/// Extract action for resource-groups paths
/// Matches seeded permissions (api:resource-groups:*) under resource "resource-groups"
fn extract_resource_groups_action(segments: &[&str], method: &str) -> Option<String> {
    let action = match (segments.len(), method) {
        // /api/clusters/resource-groups
        (2, "GET") => "list",
        (2, "POST") => "create",
        // /api/clusters/resource-groups/usage | analysis | :name
        (3, "GET") => match *segments.get(2)? {
            "usage" => "usage",
            "analysis" => "analysis",
            _ => "get",
        },
        (3, "PUT") => "update",
        (3, "DELETE") => "delete",
        _ => return None,
    };

    Some(action.to_string())
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
