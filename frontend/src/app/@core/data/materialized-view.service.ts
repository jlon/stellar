import { Injectable } from '@angular/core';
import { Observable } from 'rxjs';
import { ApiService } from './api.service';

export interface MaterializedView {
  id?: number;
  name: string;
  database: string;
  database_name?: string; // Alias for database
  query: string;
  status: string;
  rows?: number;
  created_time?: string;
  last_refresh_time?: string;
  last_refresh_finished_time?: string;
  last_refresh_state?: string;
  last_refresh_error_message?: string;
  refresh_type?: string;
  is_active?: boolean;
  refresh_status?: string;
  partition_type?: string;
}

export interface MaterializedViewDDL {
  ddl: string;
}

export interface CreateMaterializedViewRequest {
  name?: string;
  database?: string;
  sql: string;
  properties?: Record<string, string>;
  [key: string]: any; // Allow any additional fields
}

export interface RefreshMaterializedViewRequest {
  priority?: 'LOW' | 'NORMAL' | 'HIGH';
  mode?: string;
  force?: boolean;
  partition_start?: string;
  partition_end?: string;
  [key: string]: any; // Allow any additional fields
}

export interface AlterMaterializedViewRequest {
  alter_clause: string;
}

@Injectable({
  providedIn: 'root',
})
export class MaterializedViewService {
  constructor(private api: ApiService) {}

  // All methods now use backend routes without cluster ID
  // The active cluster is determined by the backend

  getMaterializedViews(database?: string): Observable<MaterializedView[]> {
    const params = database ? { database } : {};
    return this.api.get<MaterializedView[]>(
      `/clusters/materialized_views`,
      params
    );
  }

  getMaterializedView(mvName: string): Observable<MaterializedView> {
    return this.api.get<MaterializedView>(
      `/clusters/materialized_views/${mvName}`
    );
  }

  getMaterializedViewDDL(mvName: string): Observable<MaterializedViewDDL> {
    return this.api.get<MaterializedViewDDL>(
      `/clusters/materialized_views/${mvName}/ddl`
    );
  }

  createMaterializedView(
    request: CreateMaterializedViewRequest
  ): Observable<any> {
    return this.api.post(`/clusters/materialized_views`, request);
  }

  deleteMaterializedView(
    mvName: string,
    ifExists: boolean = true
  ): Observable<any> {
    return this.api.delete(
      `/clusters/materialized_views/${mvName}?if_exists=${ifExists}`
    );
  }

  refreshMaterializedView(
    mvName: string,
    request: RefreshMaterializedViewRequest
  ): Observable<any> {
    return this.api.post(
      `/clusters/materialized_views/${mvName}/refresh`,
      request
    );
  }

  cancelRefreshMaterializedView(
    mvName: string,
    force: boolean = false
  ): Observable<any> {
    return this.api.post(
      `/clusters/materialized_views/${mvName}/cancel?force=${force}`,
      {}
    );
  }

  alterMaterializedView(
    mvName: string,
    request: AlterMaterializedViewRequest
  ): Observable<any> {
    return this.api.put(
      `/clusters/materialized_views/${mvName}`,
      request
    );
  }
}
