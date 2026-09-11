import { Injectable, inject } from '@angular/core';
import { HttpClient, HttpParams } from '@angular/common/http';
import { Observable } from 'rxjs';

import {
  ResourceGroup,
  CreateResourceGroupRequest,
  UpdateResourceGroupRequest,
  ResourceGroupUsage,
  ResourceUsageAnalysis,
} from './models/resource-group.model';

@Injectable({
  providedIn: 'root',
})
export class ResourceGroupService {
  private http = inject(HttpClient);

  private readonly apiUrl = '/api/clusters/resource-groups';

  getResourceGroups(): Observable<ResourceGroup[]> {
    return this.http.get<ResourceGroup[]>(this.apiUrl);
  }

  getResourceGroup(name: string): Observable<ResourceGroup> {
    return this.http.get<ResourceGroup>(`${this.apiUrl}/${encodeURIComponent(name)}`);
  }

  createResourceGroup(request: CreateResourceGroupRequest): Observable<void> {
    return this.http.post<void>(this.apiUrl, request);
  }

  updateResourceGroup(name: string, request: UpdateResourceGroupRequest): Observable<void> {
    return this.http.put<void>(`${this.apiUrl}/${encodeURIComponent(name)}`, request);
  }

  deleteResourceGroup(name: string): Observable<void> {
    return this.http.delete<void>(`${this.apiUrl}/${encodeURIComponent(name)}`);
  }

  getResourceGroupUsage(): Observable<ResourceGroupUsage[]> {
    return this.http.get<ResourceGroupUsage[]>(`${this.apiUrl}/usage`);
  }

  getResourceUsageAnalysis(days: number = 30): Observable<ResourceUsageAnalysis> {
    const params = new HttpParams().set('days', days.toString());
    return this.http.get<ResourceUsageAnalysis>(`${this.apiUrl}/analysis`, { params });
  }
}