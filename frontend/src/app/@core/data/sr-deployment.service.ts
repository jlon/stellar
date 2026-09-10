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
  created_at: string;
}

export interface DeploymentNode {
  host_id: number;
  advertise_host: string;
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
}
