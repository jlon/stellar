# Stellar

[English](README.md) | [简体中文](README.zh-CN.md)

> An operations control plane for StarRocks and Apache Doris OLAP clusters.

Stellar brings clusters, nodes, queries, permissions, and audit records into one console. It also provides Query Profile diagnostics, capacity forecasting, alerting, and AI-assisted operations that require human approval.

[Quick Start](#quick-start) · [Deployment Guide](docs/deploy/DEPLOYMENT_GUIDE.md) · [Releases](https://github.com/jlon/stellar/releases) · [Configuration and Operations](docs/deploy/DEPLOYMENT_GUIDE.md) · [License](LICENSE)

<p align="center">
  <img src="docs/images/v2/集群概览.png" alt="Stellar cluster overview" width="100%">
</p>

## What It Does

| Capability | Description |
| --- | --- |
| Multi-cluster operations | Manage StarRocks and Doris clusters from one place. Inspect FE and BE/CN node health, resource metrics, and capacity trends. |
| Query diagnostics | Investigate live queries, run SQL, inspect audit logs, and visualize Query Profiles to identify bottlenecks and recommendations. |
| Controlled AI operations | Gather evidence, diagnose incidents, and forecast capacity from real cluster data. Every action requires human approval and is audited. |
| Security and governance | Manage organizations, users, roles, resource groups, permission requests, and operational audit records for multi-team environments. |

## Screenshots

<table>
  <tr>
    <td width="50%"><img src="docs/images/v2/智能运维助手.png" alt="Operations assistant"><br><b>Operations Assistant</b><br>Answers questions from cluster evidence and produces auditable recommendations and controlled actions.</td>
    <td width="50%"><img src="docs/images/v2/profile可视化.png" alt="Query Profile diagnostics"><br><b>Query Profile Diagnostics</b><br>Locates bottlenecks in the execution DAG and presents root-cause paths with optimization recommendations.</td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/images/v2/实时查询.png" alt="SQL workspace"><br><b>SQL Workspace</b><br>Browse catalogs, run SQL, and inspect results, charts, and execution history.</td>
    <td width="50%"><img src="docs/images/v2/权限管理.png" alt="Permissions and audit"><br><b>Permissions and Audit</b><br>Manage access by organization and role, and retain records of important operations.</td>
  </tr>
</table>

## Quick Start

The quickest way to run Stellar is with Docker. The first startup generates a one-time password for `admin`; change it after signing in.

```bash
docker run -d \
  --name stellar \
  --restart unless-stopped \
  -p 9527:9527 \
  -v "$(pwd)/stellar-data:/data" \
  ghcr.io/jlon/stellar:latest

docker logs stellar 2>&1 | grep 'password:'
```

Open `http://localhost:9527` and sign in with `admin` and the one-time password from the logs.

See the [Deployment Guide](docs/deploy/DEPLOYMENT_GUIDE.md) for production deployment, DEB packages, static binaries, Docker Compose, Kubernetes, data directories, and upgrades.

## Develop From Source

Requirements: Rust 1.75+, Node.js and npm, and Docker when developing with Docker.

```bash
git clone https://github.com/jlon/stellar.git
cd stellar

# Terminal 1: backend development server
make dev-backend

# Terminal 2: frontend development server
make dev-frontend
```

`make dev-backend` explicitly sets `STELLAR_ENV=development`. A new local data directory uses `admin/admin`; existing `admin/admin` seed data remains valid, and initialized accounts are not reset when the service restarts. The development backend listens on `127.0.0.1:8081`; the frontend runs at `http://localhost:4200`.

Build static binaries and DEB/npm packages with:

```bash
make build
```

See the [release process](docs/RELEASE_PROCESS.md) and available [Makefile](Makefile) targets for details.

## Before Adding a Cluster

Create a least-privilege monitoring account for every managed StarRocks or Doris cluster. Do not use `root`. For StarRocks, run the repository-provided initialization script:

```bash
mysql -h <fe_host> -P 9030 -u root -p \
  < scripts/permissions/setup_stellar_role.sql
```

See the [permission script guide](scripts/permissions/README_PERMISSIONS.md) for the required privileges and verification steps.

## Technology Stack

- Backend: Rust, Axum, and SQLx. Platform metadata supports SQLite, MySQL/MariaDB, and PostgreSQL.
- Frontend: Angular, Nebular, and ECharts.
- Distribution: musl static binaries, Docker, Kubernetes, and Debian packages.

## Documentation and Contributions

- [Deployment Guide](docs/deploy/DEPLOYMENT_GUIDE.md)
- [Query Profile diagnostic design](docs/profile/profile-diagnostic-system-review.md)
- [Operations assistant design](docs/agent/ops-agent-design.md)
- [Changelog](CHANGELOG.md)
- [Open a pull request](https://github.com/jlon/stellar/pulls)

Report issues and improvement ideas through [GitHub Issues](https://github.com/jlon/stellar/issues). Before submitting code, run the relevant module tests and lint checks.

## License

Stellar is released under the [Apache License 2.0](LICENSE).
