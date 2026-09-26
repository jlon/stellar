import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';

import { ApiService } from './api.service';

export type MaterializedViewKind = 'async' | 'rollup';
export type RefreshMode = 'async' | 'sync' | 'auto' | 'complete';
export type MaterializedViewState = 'active' | 'inactive';

export interface MaterializedViewRef {
  database: string;
  name: string;
  kind: MaterializedViewKind;
}

export interface MaterializedView {
  id: string;
  name: string;
  database_name: string;
  kind: MaterializedViewKind;
  definition: string;
  refresh_type: string;
  is_active: boolean;
  partition_type?: string;
  task_id?: string;
  task_name?: string;
  last_refresh_start_time?: string;
  last_refresh_finished_time?: string;
  last_refresh_duration?: string;
  last_refresh_state?: string;
  last_refresh_error_code?: string;
  last_refresh_error_message?: string;
  last_refresh_start_partition?: string;
  last_refresh_end_partition?: string;
  last_refresh_force_refresh?: boolean;
  rows?: number;
}

export interface MaterializedViewDDL {
  object: MaterializedViewRef;
  ddl: string;
}

export interface DependencyObject {
  catalog?: string;
  database?: string;
  name: string;
  kind: 'table' | 'view' | 'materialized_view' | 'unknown';
}

export interface MaterializedViewDependency {
  object: DependencyObject;
  evidence: 'verified' | 'partial' | 'annotated' | 'unknown';
  source: 'star_rocks_object_dependencies' | 'rollup_parent' | 'doris_definition';
  evidence_snippet?: string;
}

export interface MaterializedViewDependencies {
  object: MaterializedViewRef;
  dependencies: MaterializedViewDependency[];
  complete: boolean;
  warnings: string[];
}

export interface CreateMaterializedViewRequest {
  database: string;
  name: string;
  source_database: string;
  source_table: string;
  columns: string[];
  schedule: RefreshSchedule;
}

export interface PartitionValue {
  type: 'date' | 'timestamp' | 'integer' | 'string';
  value: string | number;
}

export interface RefreshMaterializedViewRequest {
  mode: RefreshMode;
  force?: boolean;
  partition?: { start: PartitionValue; end: PartitionValue };
}

export interface RefreshSchedule {
  kind: 'manual' | 'scheduled';
  interval?: number;
  unit?: 'hour' | 'day';
}

@Injectable({ providedIn: 'root' })
export class MaterializedViewService {
  private api = inject(ApiService);

  getMaterializedViews(database?: string): Observable<MaterializedView[]> {
    return this.api.get<MaterializedView[]>('/clusters/materialized_views', database ? { database } : {});
  }

  getMaterializedView(reference: MaterializedViewRef): Observable<MaterializedView> {
    return this.api.get<MaterializedView>(this.objectPath(reference));
  }

  getMaterializedViewDDL(reference: MaterializedViewRef): Observable<MaterializedViewDDL> {
    return this.api.get<MaterializedViewDDL>(`${this.objectPath(reference)}/ddl`);
  }

  getDependencies(reference: MaterializedViewRef): Observable<MaterializedViewDependencies> {
    return this.api.get<MaterializedViewDependencies>(`${this.objectPath(reference)}/dependencies`);
  }

  createMaterializedView(request: CreateMaterializedViewRequest): Observable<unknown> {
    return this.api.post('/clusters/materialized_views', request);
  }

  deleteMaterializedView(reference: MaterializedViewRef): Observable<unknown> {
    return this.api.delete(this.objectPath(reference));
  }

  refreshMaterializedView(
    reference: MaterializedViewRef,
    request: RefreshMaterializedViewRequest,
  ): Observable<unknown> {
    return this.api.post(`${this.objectPath(reference)}/refresh`, request);
  }

  cancelRefreshMaterializedView(reference: MaterializedViewRef, force = false): Observable<unknown> {
    return this.api.post(`${this.objectPath(reference)}/cancel?force=${force}`, {});
  }

  setMaterializedViewState(reference: MaterializedViewRef, state: MaterializedViewState): Observable<unknown> {
    return this.api.put(`${this.objectPath(reference)}/state`, { state });
  }

  renameMaterializedView(reference: MaterializedViewRef, newName: string): Observable<unknown> {
    return this.api.put(`${this.objectPath(reference)}/rename`, { new_name: newName });
  }

  updateRefreshSchedule(reference: MaterializedViewRef, schedule: RefreshSchedule): Observable<unknown> {
    return this.api.put(`${this.objectPath(reference)}/refresh-schedule`, { schedule });
  }

  private objectPath(reference: MaterializedViewRef): string {
    return `/clusters/materialized_views/${encodeURIComponent(reference.database)}/${encodeURIComponent(reference.name)}/${reference.kind}`;
  }
}
