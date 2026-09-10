import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';

import { ApiService } from './api.service';

export type SrPackageStatus = 'pending' | 'cached' | 'failed';

export interface SrPackage {
  id: number;
  organization_id: number;
  version: string;
  package_url: string;
  local_path?: string | null;
  sha256: string;
  status: SrPackageStatus;
  created_at: string;
}

export interface CreateSrPackageRequest {
  organization_id?: number;
  version: string;
  package_url?: string;
  local_path?: string;
  sha256: string;
}

@Injectable({
  providedIn: 'root',
})
export class SrPackageService {
  private readonly api = inject(ApiService);

  listPackages(): Observable<SrPackage[]> {
    return this.api.get<SrPackage[]>('/sr-ops/packages');
  }

  createPackage(request: CreateSrPackageRequest): Observable<SrPackage> {
    return this.api.post<SrPackage>('/sr-ops/packages', request);
  }
}
