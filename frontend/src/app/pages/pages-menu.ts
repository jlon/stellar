import { NbMenuItem } from '@nebular/theme';

export const MENU_ITEMS: NbMenuItem[] = [
  {
    title: '集群列表',
    icon: { icon: 'list-outline', status: 'info' },
    link: '/pages/starrocks/dashboard',
    home: true,
    data: { permission: 'menu:dashboard' },
  } as NbMenuItem & { data?: { permission: string } },
  {
    title: '集群概览',
    icon: { icon: 'pie-chart-outline', status: 'success' },
    link: '/pages/starrocks/overview',
    data: { permission: 'menu:overview' },
  } as NbMenuItem & { data?: { permission: string } },
  {
    title: '节点管理',
    icon: { icon: 'hard-drive-outline', status: 'warning' },
    data: { permission: 'menu:nodes' },
    children: [
      {
        title: 'Frontend 节点',
        icon: { icon: 'monitor-outline', status: 'info' },
        link: '/pages/starrocks/frontends',
        data: { permission: 'menu:nodes:frontends' },
      } as NbMenuItem & { data?: { permission: string } },
      {
        title: 'Backend 节点',
        icon: { icon: 'layers-outline', status: 'info' },
        link: '/pages/starrocks/backends',
        data: { permission: 'menu:nodes:backends' },
      } as NbMenuItem & { data?: { permission: string } },
    ],
  } as NbMenuItem & { data?: { permission: string } },
  {
    title: '查询管理',
    icon: { icon: 'search-outline', status: 'primary' },
    data: { permission: 'menu:queries' },
    children: [
      {
        title: '实时查询',
        icon: { icon: 'clock-outline', status: 'primary' },
        link: '/pages/starrocks/queries/execution',
        data: { permission: 'menu:queries:execution' },
      } as NbMenuItem & { data?: { permission: string } },
      {
        title: '导入任务',
        icon: { icon: 'upload-outline', status: 'primary' },
        link: '/pages/starrocks/loads',
        data: { permission: 'menu:queries:execution' },
      } as NbMenuItem & { data?: { permission: string } },
      {
        title: 'Profiles',
        icon: { icon: 'file-text-outline', status: 'primary' },
        link: '/pages/starrocks/queries/profiles',
        data: { permission: 'menu:queries:profiles' },
      } as NbMenuItem & { data?: { permission: string } },
      {
        title: '审计日志',
        icon: { icon: 'archive-outline', status: 'primary' },
        link: '/pages/starrocks/queries/audit-logs',
        data: { permission: 'menu:queries:audit-logs' },
      } as NbMenuItem & { data?: { permission: string } },
      {
        title: 'SQL黑名单',
        icon: { icon: 'slash-outline', status: 'danger' },
        link: '/pages/starrocks/queries/blacklist',
        data: { permission: 'menu:queries:blacklist' },
      } as NbMenuItem & { data?: { permission: string } },
    ],
  } as NbMenuItem & { data?: { permission: string } },
  {
    title: '物化视图',
    icon: { icon: 'cube-outline', status: 'info' },
    link: '/pages/starrocks/materialized-views',
    data: { permission: 'menu:materialized-views' },
  } as NbMenuItem & { data?: { permission: string } },
  {
    title: '功能卡片',
    icon: { icon: 'grid-outline', status: 'warning' },
    link: '/pages/starrocks/system',
    data: { permission: 'menu:system-functions' },
  } as NbMenuItem & { data?: { permission: string } },
  {
    title: '会话管理',
    icon: { icon: 'person-outline', status: 'success' },
    link: '/pages/starrocks/sessions',
    data: { permission: 'menu:sessions' },
  } as NbMenuItem & { data?: { permission: string } },
  {
    title: '变量管理',
    icon: { icon: 'options-2-outline', status: 'primary' },
    link: '/pages/starrocks/variables',
    data: { permission: 'menu:variables' },
  } as NbMenuItem & { data?: { permission: string } },
  {
    title: '集群运维',
    icon: { icon: 'shield-outline', status: 'danger' },
    data: { permission: 'menu:cluster-ops' },
    children: [
      {
        title: '权限管理',
        icon: { icon: 'lock-outline', status: 'danger' },
        link: '/pages/cluster-ops/permission-management',
        data: { permission: 'menu:cluster-ops:auth' },
      } as NbMenuItem & { data?: { permission: string } },
      {
        title: '资源组管理',
        icon: { icon: 'funnel-outline', status: 'danger' },
        link: '/pages/cluster-ops/resource-groups',
        data: { permission: 'menu:cluster-ops:resource-groups' },
      } as NbMenuItem & { data?: { permission: string } },
    ],
  } as NbMenuItem & { data?: { permission: string } },
  {
    title: '智能运维',
    icon: { icon: 'flash-outline', status: 'primary' },
    data: { permission: 'menu:agent' },
    children: [
      {
        title: '智能运维助手',
        icon: { icon: 'radio-outline', status: 'primary' },
        link: '/pages/cluster-ops/agent',
        data: { permission: 'menu:agent' },
      } as NbMenuItem & { data?: { permission: string } },
    ],
  } as NbMenuItem & { data?: { permission: string } },
  {
    title: '系统管理',
    icon: { icon: 'settings-2-outline', status: 'info' },
    data: { permission: 'menu:system' }, // Parent menu permission
    children: [
      {
        title: '用户管理',
        icon: { icon: 'person-outline', status: 'info' },
        link: '/pages/system/users',
        data: { permission: 'menu:system:users' },
      } as NbMenuItem & { data?: { permission: string } },
      {
        title: '角色管理',
        icon: { icon: 'pricetags-outline', status: 'info' },
        link: '/pages/system/roles',
        data: { permission: 'menu:system:roles' },
      } as NbMenuItem & { data?: { permission: string } },
      {
        title: '组织管理',
        icon: { icon: 'briefcase-outline', status: 'info' },
        link: '/pages/system/organizations',
        data: { permission: 'menu:system:organizations' },
      } as NbMenuItem & { data?: { permission: string } },
      {
        title: '操作日志',
        icon: { icon: 'clock-outline', status: 'info' },
        link: '/pages/system/op-audit',
        data: { permission: 'menu:system:op-audit' },
      } as NbMenuItem & { data?: { permission: string } },
      {
        title: 'LLM管理',
        icon: { icon: 'code-outline', status: 'info' },
        link: '/pages/system/llm-providers',
        data: { permission: 'menu:system:llm' },
      } as NbMenuItem & { data?: { permission: string } },
    ],
  } as NbMenuItem & { data?: { permission: string } },
];
