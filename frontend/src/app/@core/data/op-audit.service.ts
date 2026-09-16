import { Injectable, inject } from '@angular/core';
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

  list(params?: { target_type?: string; action?: string; limit?: number }): Observable<{ items: OpAuditLog[] }> {
    return this.api.get<{ items: OpAuditLog[] }>('/op-audit-logs', params as Record<string, string>);
  }
}
