import { Component, Input, Output, EventEmitter, OnInit, OnDestroy } from '@angular/core';
import { Subject, of } from 'rxjs';
import { takeUntil, debounceTime, distinctUntilChanged } from 'rxjs/operators';
import { CascadeSelectorCacheService } from '../../../../@core/services/cascade-selector-cache.service';
import { NbToastrService } from '@nebular/theme';

/**
 * Optimized Cascade Selector Component
 * Provides cached and optimized catalog/database/table selection
 * with performance improvements and better UX
 *
 * NOTE: Currently using mock data for demonstration.
 * In production, replace with actual API calls.
 */
@Component({
  selector: 'ngx-cascade-selector',
  template: `
    <div class="cascade-selector-container">
      <!-- Catalog Selector -->
      <div class="selector-group" *ngIf="showCatalog">
        <label>Catalog</label>
        <nb-select
          [(selected)]="selectedCatalog"
          placeholder="选择 Catalog"
          [disabled]="!clusterId || catalogLoading"
          (selectedChange)="onCatalogChange($event)"
          fullWidth>
          <nb-option value="">全部</nb-option>
          <nb-option *ngFor="let catalog of catalogs" [value]="catalog">
            {{ catalog }}
          </nb-option>
        </nb-select>
        <nb-spinner *ngIf="catalogLoading" size="tiny" status="primary"></nb-spinner>
      </div>

      <!-- Database Selector -->
      <div class="selector-group" *ngIf="showDatabase">
        <label>Database</label>
        <nb-select
          [(selected)]="selectedDatabase"
          placeholder="选择 Database"
          [disabled]="!clusterId || (!selectedCatalog && requireCatalog) || databaseLoading"
          (selectedChange)="onDatabaseChange($event)"
          fullWidth>
          <nb-option value="">全部</nb-option>
          <nb-option *ngFor="let database of databases" [value]="database">
            {{ database }}
          </nb-option>
        </nb-select>
        <nb-spinner *ngIf="databaseLoading" size="tiny" status="primary"></nb-spinner>
      </div>

      <!-- Table Selector -->
      <div class="selector-group" *ngIf="showTable">
        <label>Table</label>
        <nb-select
          [(selected)]="selectedTable"
          placeholder="选择 Table"
          [disabled]="!clusterId || !selectedDatabase || tableLoading"
          (selectedChange)="onTableChange($event)"
          fullWidth>
          <nb-option value="">全部</nb-option>
          <nb-option *ngFor="let table of tables" [value]="table">
            {{ table }}
          </nb-option>
        </nb-select>
        <nb-spinner *ngIf="tableLoading" size="tiny" status="primary"></nb-spinner>
      </div>

      <!-- Clear Button -->
      <div class="selector-actions" *ngIf="showClearButton">
        <button nbButton ghost status="basic" size="small" (click)="clearSelection()">
          <nb-icon icon="refresh-outline"></nb-icon>
          清空选择
        </button>
      </div>
    </div>
  `,
  styles: [`
    .cascade-selector-container {
      display: flex;
      flex-direction: column;
      gap: 1rem;
    }

    .selector-group {
      position: relative;

      label {
        display: block;
        margin-bottom: 0.5rem;
        font-weight: 500;
        color: var(--text-hint-color);
      }

      nb-spinner {
        position: absolute;
        right: 2.5rem;
        top: 50%;
        transform: translateY(-50%);
      }
    }

    .selector-actions {
      display: flex;
      justify-content: flex-end;
      margin-top: 0.5rem;
    }

    :host ::ng-deep nb-select {
      .select-button {
        padding-right: 3rem;
      }
    }
  `],
})
export class CascadeSelectorComponent implements OnInit, OnDestroy {
  @Input() clusterId: number;
  @Input() showCatalog: boolean = true;
  @Input() showDatabase: boolean = true;
  @Input() showTable: boolean = true;
  @Input() requireCatalog: boolean = true;
  @Input() showClearButton: boolean = true;
  @Input() autoLoadCatalogs: boolean = true;
  @Input() cacheTTL: number = 5 * 60 * 1000; // 5 minutes

  @Output() selectionChange = new EventEmitter<{
    catalog?: string;
    database?: string;
    table?: string;
  }>();

  // Selected values
  selectedCatalog: string = '';
  selectedDatabase: string = '';
  selectedTable: string = '';

  // Data arrays
  catalogs: string[] = [];
  databases: string[] = [];
  tables: string[] = [];

  // Loading states
  catalogLoading = false;
  databaseLoading = false;
  tableLoading = false;

  private destroy$ = new Subject<void>();

  constructor(
    private cacheService: CascadeSelectorCacheService,
    private toastr: NbToastrService,
  ) {}

  ngOnInit(): void {
    if (this.autoLoadCatalogs && this.clusterId) {
      this.loadCatalogs();
    }
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  /**
   * Load catalogs with caching (using mock data for now)
   */
  private loadCatalogs(): void {
    if (!this.clusterId) return;

    const cacheKey = this.cacheService.generateCacheKey(this.clusterId, 'catalog');
    this.catalogLoading = true;

    // Mock data for demonstration
    const mockCatalogs = ['default', 'hive', 'iceberg', 'jdbc'];

    this.cacheService.getOrFetch(
      cacheKey,
      () => of(mockCatalogs),
      this.cacheTTL
    ).pipe(
      takeUntil(this.destroy$)
    ).subscribe({
      next: (catalogs) => {
        this.catalogs = catalogs || [];
        this.catalogLoading = false;

        // Auto-select if only one option
        if (this.catalogs.length === 1) {
          this.selectedCatalog = this.catalogs[0];
          this.onCatalogChange(this.selectedCatalog);
        }
      },
      error: (err) => {
        console.error('Failed to load catalogs:', err);
        this.toastr.danger('加载 Catalog 列表失败', '错误');
        this.catalogLoading = false;
      },
    });
  }

  /**
   * Load databases with caching (using mock data for now)
   */
  private loadDatabases(): void {
    if (!this.clusterId || (!this.selectedCatalog && this.requireCatalog)) return;

    const cacheKey = this.cacheService.generateCacheKey(
      this.clusterId,
      'database',
      this.selectedCatalog
    );
    this.databaseLoading = true;

    // Mock data based on catalog
    const mockDatabases = this.selectedCatalog === 'default' ?
      ['information_schema', 'my_database', 'test_db', 'production'] :
      ['db1', 'db2', 'db3'];

    this.cacheService.getOrFetch(
      cacheKey,
      () => of(mockDatabases),
      this.cacheTTL
    ).pipe(
      takeUntil(this.destroy$)
    ).subscribe({
      next: (databases) => {
        this.databases = databases || [];
        this.databaseLoading = false;

        // Auto-select if only one option
        if (this.databases.length === 1) {
          this.selectedDatabase = this.databases[0];
          this.onDatabaseChange(this.selectedDatabase);
        }
      },
      error: (err) => {
        console.error('Failed to load databases:', err);
        this.toastr.danger('加载 Database 列表失败', '错误');
        this.databaseLoading = false;
      },
    });
  }

  /**
   * Load tables with caching (using mock data for now)
   */
  private loadTables(): void {
    if (!this.clusterId || !this.selectedDatabase) return;

    const cacheKey = this.cacheService.generateCacheKey(
      this.clusterId,
      'table',
      `${this.selectedCatalog || 'default'}:${this.selectedDatabase}`
    );
    this.tableLoading = true;

    // Mock data based on database
    const mockTables = [
      'users', 'orders', 'products', 'transactions',
      'logs', 'analytics', 'reports', 'metrics'
    ].filter(() => Math.random() > 0.3); // Random subset

    this.cacheService.getOrFetch(
      cacheKey,
      () => of(mockTables),
      this.cacheTTL
    ).pipe(
      takeUntil(this.destroy$)
    ).subscribe({
      next: (tables) => {
        this.tables = tables || [];
        this.tableLoading = false;

        // Auto-select if only one option
        if (this.tables.length === 1) {
          this.selectedTable = this.tables[0];
          this.onTableChange(this.selectedTable);
        }
      },
      error: (err) => {
        console.error('Failed to load tables:', err);
        this.toastr.danger('加载 Table 列表失败', '错误');
        this.tableLoading = false;
      },
    });
  }

  /**
   * Handle catalog change
   */
  onCatalogChange(value: string): void {
    this.selectedCatalog = value;

    // Clear downstream selections
    this.selectedDatabase = '';
    this.selectedTable = '';
    this.databases = [];
    this.tables = [];

    // Invalidate related cache
    if (value) {
      this.cacheService.invalidateRelatedCache(this.clusterId, 'catalog');
      this.loadDatabases();
    }

    this.emitSelectionChange();
  }

  /**
   * Handle database change
   */
  onDatabaseChange(value: string): void {
    this.selectedDatabase = value;

    // Clear downstream selections
    this.selectedTable = '';
    this.tables = [];

    // Invalidate related cache
    if (value) {
      this.cacheService.invalidateRelatedCache(this.clusterId, 'database');
      this.loadTables();
    }

    this.emitSelectionChange();
  }

  /**
   * Handle table change
   */
  onTableChange(value: string): void {
    this.selectedTable = value;
    this.emitSelectionChange();
  }

  /**
   * Clear all selections
   */
  clearSelection(): void {
    this.selectedCatalog = '';
    this.selectedDatabase = '';
    this.selectedTable = '';
    this.databases = [];
    this.tables = [];

    this.emitSelectionChange();
  }

  /**
   * Emit selection change event
   */
  private emitSelectionChange(): void {
    this.selectionChange.emit({
      catalog: this.selectedCatalog,
      database: this.selectedDatabase,
      table: this.selectedTable,
    });
  }

  /**
   * Refresh catalogs (public method for parent component)
   */
  refreshCatalogs(): void {
    const cacheKey = this.cacheService.generateCacheKey(this.clusterId, 'catalog');
    this.cacheService.clearCache(cacheKey);
    this.loadCatalogs();
  }

  /**
   * Set initial values (public method for parent component)
   */
  setValues(catalog?: string, database?: string, table?: string): void {
    if (catalog) {
      this.selectedCatalog = catalog;
      this.onCatalogChange(catalog);
    }

    if (database) {
      setTimeout(() => {
        this.selectedDatabase = database;
        this.onDatabaseChange(database);
      }, 100);
    }

    if (table) {
      setTimeout(() => {
        this.selectedTable = table;
      }, 200);
    }
  }
}