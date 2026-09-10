import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';

import { ApiService } from './api.service';

export interface DeploymentCredential {
  id: number;
  organization_id: number;
  name: string;
  username: string;
  created_at: string;
}

export interface DeploymentTask {
  id: number;
  managed_cluster_id: number;
  status: string;
  current_step?: string;
  error_message?: string;
  result_json?: string;
  created_at: string;
  started_at?: string;
  finished_at?: string;
}

export interface DeploymentEvent {
  id: number;
  step: string;
  status: string;
  message: string;
  created_at: string;
}

export interface DeploymentTaskDetail extends DeploymentTask {
  events: DeploymentEvent[];
}

export interface ManagedCluster {
  id: number;
  name: string;
  sr_version: string;
  status: string;
  cluster_id?: number;
  install_dir?: string;
  created_at: string;
}

export interface ManagedClusterNode {
  id: number;
  host_id: number;
  role: 'fe' | 'be';
  fe_role?: 'leader' | 'follower';
  advertise_host: string;
  service_port: number;
  status: string;
}

export interface ManagedClusterDetail extends ManagedCluster {
  nodes: ManagedClusterNode[];
}

export interface ConfigRevisionSummary {
  id: number;
  node_id: number;
  revision: number;
  content_sha256: string;
  task_id?: number;
  created_at: string;
}

export interface ConfigRevision extends ConfigRevisionSummary {
  content: string;
}

export interface ConfigDiffLine {
  kind: 'context' | 'added' | 'removed';
  text: string;
}

export interface NodeLogsResponse {
  node_id: number;
  logs: string;
}

export interface DeploymentNode {
  host_id: number;
  advertise_host: string;
  edit_log_port?: number;
  http_port?: number;
  query_port?: number;
  rpc_port?: number;
  heartbeat_port?: number;
  be_port?: number;
  webserver_port?: number;
  brpc_port?: number;
  starlet_port?: number;
  meta_dir?: string;
  storage_dir?: string;
}

export interface CreateDeploymentRequest {
  name: string;
  package_id: number;
  ssh_credential_id: number;
  operator_credential_id: number;
  install_dir: string;
  confirm_non_ha: boolean;
  frontends: DeploymentNode[];
  backends: DeploymentNode[];
}

@Injectable({ providedIn: 'root' })
export class SrDeploymentService {
  private readonly api = inject(ApiService);

  listSshCredentials(): Observable<DeploymentCredential[]> {
    return this.api.get<DeploymentCredential[]>('/sr-ops/credentials');
  }

  createSshCredential(request: { name: string; username: string; private_key: string }): Observable<DeploymentCredential> {
    return this.api.post<DeploymentCredential>('/sr-ops/credentials', request);
  }

  listDatabaseCredentials(): Observable<DeploymentCredential[]> {
    return this.api.get<DeploymentCredential[]>('/sr-ops/database-credentials');
  }

  createDatabaseCredential(request: { name: string; username: string; password: string }): Observable<DeploymentCredential> {
    return this.api.post<DeploymentCredential>('/sr-ops/database-credentials', request);
  }

  createDeployment(request: CreateDeploymentRequest): Observable<DeploymentTask> {
    return this.api.post<DeploymentTask>('/sr-ops/deployments', request);
  }

  createReadOnlyAdoption(request: {
    name: string;
    fe_host: string;
    fe_http_port: number;
    fe_query_port: number;
    operator_credential_id: number;
  }): Observable<DeploymentTask> {
    return this.api.post<DeploymentTask>('/sr-ops/adoptions', request);
  }

  listTasks(): Observable<DeploymentTask[]> {
    return this.api.get<DeploymentTask[]>('/sr-ops/tasks');
  }

  getTask(id: number): Observable<DeploymentTaskDetail> {
    return this.api.get<DeploymentTaskDetail>(`/sr-ops/tasks/${id}`);
  }

  listClusters(): Observable<ManagedCluster[]> {
    return this.api.get<ManagedCluster[]>('/sr-ops/clusters');
  }

  getCluster(id: number): Observable<ManagedClusterDetail> {
    return this.api.get<ManagedClusterDetail>(`/sr-ops/clusters/${id}`);
  }

  importCluster(id: number): Observable<DeploymentTask> {
    return this.api.post<DeploymentTask>(`/sr-ops/clusters/${id}/import`, {});
  }

  submitNodeCommand(
    clusterId: number,
    nodeId: number,
    action: 'start' | 'stop' | 'restart',
  ): Observable<DeploymentTask> {
    return this.api.post<DeploymentTask>(`/sr-ops/clusters/${clusterId}/nodes/${nodeId}/commands`, { action });
  }

  readNodeLogs(clusterId: number, nodeId: number, file: string, lines = 200): Observable<NodeLogsResponse> {
    return this.api.get<NodeLogsResponse>(
      `/sr-ops/clusters/${clusterId}/nodes/${nodeId}/logs?file=${encodeURIComponent(file)}&lines=${lines}`,
    );
  }

  listConfigRevisions(clusterId: number, nodeId: number): Observable<ConfigRevisionSummary[]> {
    return this.api.get<ConfigRevisionSummary[]>(`/sr-ops/clusters/${clusterId}/configs/${nodeId}`);
  }

  getConfigRevision(clusterId: number, nodeId: number, revision: number): Observable<ConfigRevision> {
    return this.api.get<ConfigRevision>(`/sr-ops/clusters/${clusterId}/configs/${nodeId}/revisions/${revision}`);
  }

  diffConfigRevisions(
    clusterId: number,
    nodeId: number,
    from: number,
    to: number,
  ): Observable<ConfigDiffLine[]> {
    return this.api.get<ConfigDiffLine[]>(
      `/sr-ops/clusters/${clusterId}/configs/${nodeId}/diff?from=${from}&to=${to}`,
    );
  }
}
