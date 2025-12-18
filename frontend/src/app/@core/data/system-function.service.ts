import { Injectable } from '@angular/core';
import { Observable } from 'rxjs';
import { ApiService } from './api.service';
import { SystemFunction, CreateFunctionRequest, UpdateOrderRequest } from './system-function';

@Injectable({
  providedIn: 'root'
})
export class SystemFunctionService {

  constructor(private api: ApiService) {}

  // Get all custom functions
  getFunctions(): Observable<SystemFunction[]> {
    return this.api.get<SystemFunction[]>(`/clusters/system-functions`);
  }

  // Create a custom function
  createFunction(req: CreateFunctionRequest): Observable<SystemFunction> {
    return this.api.post<SystemFunction>(`/clusters/system-functions`, req);
  }

  // Execute custom function SQL
  executeFunction(functionId: number): Observable<any> {
    return this.api.post<any>(`/clusters/system-functions/${functionId}/execute`, {});
  }

  // Update sorting and category order
  updateOrders(orders: UpdateOrderRequest): Observable<void> {
    return this.api.put<void>(`/clusters/system-functions/orders`, orders);
  }

  // Toggle favorite status
  toggleFavorite(functionId: number): Observable<SystemFunction> {
    return this.api.put<SystemFunction>(`/clusters/system-functions/${functionId}/favorite`, {});
  }

  // Delete custom function
  deleteFunction(functionId: number): Observable<void> {
    return this.api.delete<void>(`/clusters/system-functions/${functionId}`);
  }

  // Update system function access time
  updateSystemFunctionAccessTime(functionName: string): Observable<void> {
    return this.api.put<void>(`/system-functions/${functionName}/access-time`, {});
  }

  // Delete category
  deleteCategory(categoryName: string): Observable<void> {
    return this.api.delete<void>(`/system-functions/category/${encodeURIComponent(categoryName)}`);
  }

  // Update function
  updateFunction(functionId: number, request: CreateFunctionRequest): Observable<SystemFunction> {
    return this.api.put<SystemFunction>(`/clusters/system-functions/${functionId}`, request);
  }
}
