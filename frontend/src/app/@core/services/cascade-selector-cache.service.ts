import { Injectable } from '@angular/core';
import { Observable, of, BehaviorSubject } from 'rxjs';
import { map, catchError, tap, shareReplay } from 'rxjs/operators';

/**
 * Cache Service for Cascading Selectors
 * Provides caching mechanism for catalog/database/table data
 * to avoid repeated API calls and improve performance
 */
@Injectable({
  providedIn: 'root',
})
export class CascadeSelectorCacheService {
  // Cache storage with TTL
  private cache = new Map<string, {
    data: any;
    timestamp: number;
    observable?: Observable<any>;
  }>();

  // Cache configuration
  private readonly DEFAULT_TTL = 5 * 60 * 1000; // 5 minutes
  private readonly MAX_CACHE_SIZE = 100;

  // Loading states
  private loadingStates = new Map<string, BehaviorSubject<boolean>>();

  /**
   * Get cached data or fetch new data
   * @param key Cache key
   * @param fetcher Function to fetch data if not cached
   * @param ttl Time to live in milliseconds
   */
  getOrFetch<T>(
    key: string,
    fetcher: () => Observable<T>,
    ttl: number = this.DEFAULT_TTL
  ): Observable<T> {
    // Check if data exists in cache and is not expired
    const cached = this.cache.get(key);
    if (cached && !this.isExpired(cached.timestamp, ttl)) {
      return of(cached.data);
    }

    // Check if request is already in progress
    if (cached?.observable) {
      return cached.observable;
    }

    // Create new request with caching
    const observable = fetcher().pipe(
      tap(data => {
        // Store in cache
        this.setCache(key, data);

        // Clear the in-progress observable
        const existing = this.cache.get(key);
        if (existing) {
          delete existing.observable;
        }
      }),
      catchError(error => {
        // Clear the in-progress observable on error
        const existing = this.cache.get(key);
        if (existing) {
          delete existing.observable;
        }
        throw error;
      }),
      shareReplay(1) // Share the observable among multiple subscribers
    );

    // Mark as in-progress
    const existingCache = this.cache.get(key);
    if (existingCache) {
      existingCache.observable = observable;
    } else {
      this.cache.set(key, {
        data: null,
        timestamp: Date.now(),
        observable
      });
    }

    return observable;
  }

  /**
   * Get cached data without fetching
   */
  get<T>(key: string): T | null {
    const cached = this.cache.get(key);
    if (cached && !this.isExpired(cached.timestamp, this.DEFAULT_TTL)) {
      return cached.data;
    }
    return null;
  }

  /**
   * Set cache data
   */
  setCache(key: string, data: any): void {
    // Implement LRU eviction if cache is too large
    if (this.cache.size >= this.MAX_CACHE_SIZE) {
      const firstKey = this.cache.keys().next().value;
      this.cache.delete(firstKey);
    }

    this.cache.set(key, {
      data,
      timestamp: Date.now(),
    });
  }

  /**
   * Clear specific cache entry
   */
  clearCache(key: string): void {
    this.cache.delete(key);
  }

  /**
   * Clear all cache entries matching a pattern
   */
  clearCachePattern(pattern: string): void {
    const regex = new RegExp(pattern);
    const keysToDelete: string[] = [];

    this.cache.forEach((_, key) => {
      if (regex.test(key)) {
        keysToDelete.push(key);
      }
    });

    keysToDelete.forEach(key => this.cache.delete(key));
  }

  /**
   * Clear all cache
   */
  clearAll(): void {
    this.cache.clear();
    this.loadingStates.clear();
  }

  /**
   * Check if cache entry is expired
   */
  private isExpired(timestamp: number, ttl: number): boolean {
    return Date.now() - timestamp > ttl;
  }

  /**
   * Get loading state for a key
   */
  getLoadingState(key: string): Observable<boolean> {
    if (!this.loadingStates.has(key)) {
      this.loadingStates.set(key, new BehaviorSubject<boolean>(false));
    }
    return this.loadingStates.get(key)!.asObservable();
  }

  /**
   * Set loading state for a key
   */
  setLoadingState(key: string, isLoading: boolean): void {
    if (!this.loadingStates.has(key)) {
      this.loadingStates.set(key, new BehaviorSubject<boolean>(isLoading));
    } else {
      this.loadingStates.get(key)!.next(isLoading);
    }
  }

  /**
   * Generate cache key for cascade selector
   */
  generateCacheKey(
    clusterId: number,
    level: 'catalog' | 'database' | 'table',
    parentValue?: string
  ): string {
    const parts = [`cluster_${clusterId}`, level];
    if (parentValue) {
      parts.push(parentValue);
    }
    return parts.join(':');
  }

  /**
   * Invalidate related cache when data changes
   * E.g., when a database is selected, invalidate table cache
   */
  invalidateRelatedCache(clusterId: number, level: 'catalog' | 'database'): void {
    if (level === 'catalog') {
      // Invalidate all database and table cache for this cluster
      this.clearCachePattern(`cluster_${clusterId}:database`);
      this.clearCachePattern(`cluster_${clusterId}:table`);
    } else if (level === 'database') {
      // Invalidate all table cache for this cluster
      this.clearCachePattern(`cluster_${clusterId}:table`);
    }
  }

  /**
   * Preload common data for better UX
   */
  preloadCommonData(
    clusterId: number,
    fetchers: {
      catalogs?: () => Observable<any>;
      databases?: (catalog: string) => Observable<any>;
    }
  ): void {
    // Preload catalogs
    if (fetchers.catalogs) {
      const catalogKey = this.generateCacheKey(clusterId, 'catalog');
      this.getOrFetch(catalogKey, fetchers.catalogs).subscribe();
    }

    // Preload default database (if default catalog exists)
    const defaultCatalog = 'default';
    if (fetchers.databases) {
      const dbKey = this.generateCacheKey(clusterId, 'database', defaultCatalog);
      this.getOrFetch(dbKey, () => fetchers.databases!(defaultCatalog)).subscribe();
    }
  }

  /**
   * Get cache statistics for monitoring
   */
  getCacheStats(): {
    size: number;
    entries: Array<{ key: string; age: number; hasData: boolean }>;
  } {
    const entries = Array.from(this.cache.entries()).map(([key, value]) => ({
      key,
      age: Date.now() - value.timestamp,
      hasData: value.data !== null,
    }));

    return {
      size: this.cache.size,
      entries,
    };
  }
}