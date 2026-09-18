import { Injectable, inject } from '@angular/core';
import { HttpClient, HttpResponse } from '@angular/common/http';
import { Observable } from 'rxjs';
import { ApiService } from './api.service';

export interface OpAuditLog {
  id: number;
  user_id: number;
  username: string;
  organization_id?: number;
  action: string;
  target_type: string;
  target_id?: number;
  target_name: string;
  created_at: string;
}

@Injectable({ providedIn: 'root' })
export class OpAuditService {
  private api = inject(ApiService);
  private http = inject(HttpClient);

  list(params?: { target_type?: string; action?: string; limit?: number }): Observable<{ items: OpAuditLog[] }> {
    return this.api.get<{ items: OpAuditLog[] }>('/op-audit-logs', params as Record<string, string>);
  }

  downloadLogArchive(): Observable<HttpResponse<Blob>> {
    return this.http.get(`${this.api.apiBaseUrl}/system/logs/archive`, {
      observe: 'response',
      responseType: 'blob',
    });
  }
}
