-- `20260917000002` 曾为只有查询菜单、没有查询 API 权限的角色补发 Load 菜单。
-- Load 页面由 API 权限守卫保护，移除这类无效菜单授权以保持可见性与可访问性一致。
DELETE rp
FROM role_permissions rp
JOIN permissions load_menu ON load_menu.id = rp.permission_id AND load_menu.code = 'menu:loads'
WHERE NOT EXISTS (
    SELECT 1
    FROM role_permissions query_rp
    JOIN permissions query_api ON query_api.id = query_rp.permission_id
    WHERE query_rp.role_id = rp.role_id
      AND query_api.code = 'api:clusters:queries'
);
