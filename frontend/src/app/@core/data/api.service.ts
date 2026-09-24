import { Injectable, inject } from '@angular/core';
import { HttpClient, HttpParams } from '@angular/common/http';
import { Observable } from 'rxjs';
import { timeout } from 'rxjs/operators';
import { environment } from '../../../environments/environment';

@Injectable({
  providedIn: 'root',
})
export class ApiService {
  private http = inject(HttpClient);

  private readonly baseUrl = environment.apiUrl;

  /** Resolved API base URL (for fetch-based streaming clients). */
  get apiBaseUrl(): string {
    return this.resolvedBaseUrl;
  }
  private readonly resolvedBaseUrl: string;

  constructor() {
    this.resolvedBaseUrl = this.computeBaseUrl(this.baseUrl);
  }

  get<T>(path: string, params?: HttpParams | Record<string, any>): Observable<T> {
    const httpParams = this.toHttpParams(params);
    return this.http.get<T>(`${this.resolvedBaseUrl}${path}`, { params: httpParams });
  }

  getBlob(path: string, params?: HttpParams | Record<string, any>): Observable<Blob> {
    const httpParams = this.toHttpParams(params);
    return this.http.get(`${this.resolvedBaseUrl}${path}`, {
      params: httpParams,
      responseType: 'blob',
    });
  }

  post<T>(path: string, body: any = {}, customTimeout?: number): Observable<T> {
    const timeoutMs = customTimeout || 650000; // Default 650 seconds (10.8 minutes), slightly longer than Nginx timeout
    return this.http.post<T>(`${this.resolvedBaseUrl}${path}`, body).pipe(
      timeout(timeoutMs),
    );
  }

  put<T>(path: string, body: any = {}): Observable<T> {
    return this.http.put<T>(`${this.resolvedBaseUrl}${path}`, body);
  }

  patch<T>(path: string, body: any = {}): Observable<T> {
    return this.http.patch<T>(`${this.resolvedBaseUrl}${path}`, body);
  }

  delete<T>(path: string): Observable<T> {
    return this.http.delete<T>(`${this.resolvedBaseUrl}${path}`);
  }

  private toHttpParams(params?: HttpParams | Record<string, any>): HttpParams | undefined {
    if (params instanceof HttpParams) {
      return params;
    }
    return params && typeof params === 'object'
      ? new HttpParams({ fromObject: params as any })
      : undefined;
  }

  private computeBaseUrl(apiUrl: string): string {
    if (!apiUrl) {
      return '';
    }
    if (/^https?:\/\//.test(apiUrl)) {
      return apiUrl;
    }
    if (apiUrl.startsWith('/')) {
      return apiUrl;
    }
    if (apiUrl.startsWith('./')) {
      const relativePath = apiUrl.substring(2);
      const basePath = this.detectBasePath();
      return this.joinPaths(basePath, relativePath);
    }
    return apiUrl;
  }

  private detectBasePath(): string {
    const path = window.location.pathname || '';
    const markers = ['/pages/', '/auth/', '/login', '/register', '/reset'];
    for (const marker of markers) {
      const index = path.indexOf(marker);
      if (index > -1) {
        const base = path.substring(0, index);
        return base.endsWith('/') ? base.slice(0, -1) : base;
      }
    }
    if (path === '/') {
      return '';
    }
    const segments = path.split('/').filter(Boolean);
    if (segments.length === 0) {
      return '';
    }
    return `/${segments[0]}`;
  }

  private joinPaths(base: string, suffix: string): string {
    const cleanedBase = base.replace(/\/+$/, '');
    const cleanedSuffix = suffix.replace(/^\/+/, '');
    if (!cleanedBase) {
      return `/${cleanedSuffix}`;
    }
    if (!cleanedSuffix) {
      return cleanedBase;
    }
    return `${cleanedBase}/${cleanedSuffix}`;
  }
}
