-- `20260917000002` 曾为只有查询菜单、没有查询 API 权限的角色补发 Load 菜单。
-- Load 页面由 API 权限守卫保护，移除这类无效菜单授权以保持可见性与可访问性一致。
DELETE FROM role_permissions
WHERE permission_id = (SELECT id FROM permissions WHERE code = 'menu:loads')
  AND role_id NOT IN (
      SELECT rp.role_id
      FROM role_permissions rp
      JOIN permissions p ON p.id = rp.permission_id
      WHERE p.code = 'api:clusters:queries'
  );
