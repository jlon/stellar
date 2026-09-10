-- StarRocks physical deployment P0: host inventory and access control.

CREATE TABLE physical_hosts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    organization_id INTEGER NOT NULL REFERENCES organizations(id),
    hostname TEXT NOT NULL,
    ssh_target TEXT NOT NULL,
    ssh_port INTEGER NOT NULL DEFAULT 22 CHECK (ssh_port BETWEEN 1 AND 65535),
    -- Algorithm and Base64 public key only; private keys never belong in this table.
    host_key TEXT NOT NULL,
    host_key_fingerprint TEXT NOT NULL,
    os_info TEXT,
    cpu_cores INTEGER,
    memory_gb INTEGER,
    disk_gb INTEGER,
    status TEXT NOT NULL DEFAULT 'unknown'
        CHECK (status IN ('online', 'offline', 'unknown')),
    labels_json TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(organization_id, ssh_target, ssh_port)
);

CREATE INDEX idx_physical_hosts_organization_id ON physical_hosts(organization_id);

CREATE TABLE sr_packages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    organization_id INTEGER NOT NULL REFERENCES organizations(id),
    version TEXT NOT NULL,
    package_url TEXT NOT NULL,
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'cached', 'failed')),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(organization_id, version, sha256)
);

CREATE INDEX idx_sr_packages_organization_id ON sr_packages(organization_id);

INSERT OR IGNORE INTO permissions (code, name, type, resource, action, description) VALUES
('menu:deployment', '部署管理', 'menu', 'deployment', 'view', '查看 StarRocks 物理机部署管理'),
('api:sr-ops:hosts:list', '查询部署主机', 'api', 'sr-ops', 'hosts:list', 'GET /api/sr-ops/hosts'),
('api:sr-ops:hosts:manage', '登记部署主机', 'api', 'sr-ops', 'hosts:manage', 'POST /api/sr-ops/hosts'),
('api:sr-ops:packages:list', '查询受控安装包', 'api', 'sr-ops', 'packages:list', 'GET /api/sr-ops/packages'),
('api:sr-ops:packages:manage', '登记受控安装包', 'api', 'sr-ops', 'packages:manage', 'POST /api/sr-ops/packages');

UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:deployment')
WHERE code IN (
    'api:sr-ops:hosts:list',
    'api:sr-ops:hosts:manage',
    'api:sr-ops:packages:list',
    'api:sr-ops:packages:manage'
);

INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
CROSS JOIN permissions p
WHERE r.code = 'super_admin'
  AND p.code IN (
      'menu:deployment',
      'api:sr-ops:hosts:list',
      'api:sr-ops:hosts:manage',
      'api:sr-ops:packages:list',
      'api:sr-ops:packages:manage'
  );

-- Existing organization administrators need the new P0 capability too. New
-- organization administrators inherit it through OrganizationService.
INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
CROSS JOIN permissions p
WHERE r.code LIKE 'org_admin_%'
  AND p.code IN (
      'menu:deployment',
      'api:sr-ops:hosts:list',
      'api:sr-ops:hosts:manage',
      'api:sr-ops:packages:list',
      'api:sr-ops:packages:manage'
  );
