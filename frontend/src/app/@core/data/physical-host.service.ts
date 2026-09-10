import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';

import { ApiService } from './api.service';

export type PhysicalHostStatus = 'online' | 'offline' | 'unknown';

export interface PhysicalHost {
  id: number;
  organization_id: number;
  hostname: string;
  ssh_target: string;
  ssh_port: number;
  host_key_fingerprint: string;
  status: PhysicalHostStatus;
  created_at: string;
  updated_at: string;
}

export interface CreatePhysicalHostRequest {
  organization_id?: number;
  hostname: string;
  ssh_target: string;
  ssh_port: number;
  host_key: string;
  host_key_fingerprint: string;
}

@Injectable({
  providedIn: 'root',
})
export class PhysicalHostService {
  private readonly api = inject(ApiService);

  listHosts(): Observable<PhysicalHost[]> {
    return this.api.get<PhysicalHost[]>('/sr-ops/hosts');
  }

  createHost(request: CreatePhysicalHostRequest): Observable<PhysicalHost> {
    return this.api.post<PhysicalHost>('/sr-ops/hosts', request);
  }
}
