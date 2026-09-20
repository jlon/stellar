import { CommonModule, DOCUMENT } from "@angular/common";
import {
  ChangeDetectionStrategy,
  ChangeDetectorRef,
  Component,
  OnDestroy,
  OnInit,
  TemplateRef,
  ViewChild,
  inject,
} from "@angular/core";
import { FormsModule } from "@angular/forms";
import {
  NbAlertModule,
  NbBadgeModule,
  NbButtonModule,
  NbCardModule,
  NbDialogModule,
  NbDialogRef,
  NbDialogService,
  NbFormFieldModule,
  NbIconModule,
  NbInputModule,
  NbListModule,
  NbOptionModule,
  NbSelectModule,
  NbSpinnerModule,
  NbToastrService,
  NbTooltipModule,
} from "@nebular/theme";
import {
  Angular2SmartTableModule,
  LocalDataSource,
  RowSelectionEvent,
} from "angular2-smart-table";
import { Subject } from "rxjs";
import { finalize, take, takeUntil, timeout } from "rxjs/operators";
import { ActivatedRoute, Router } from "@angular/router";

import { ClusterContextService } from "../../../@core/data/cluster-context.service";
import { Cluster } from "../../../@core/data/cluster.service";
import { LoadJob, LoadService } from "../../../@core/data/load.service";
import { NodeService } from "../../../@core/data/node.service";
import { ErrorHandler } from "../../../@core/utils/error-handler";
import { assignTableRows } from "../../../@core/utils/table-rows";

interface LoadTableRow {
  task: string;
  target: string;
  loadType: string;
  state: string;
  stage: string;
  dataVolume: string;
  filtered: string;
  createdAt: string;
  job: LoadJob;
}

@Component({
  selector: "ngx-load-management",
  templateUrl: "./load-management.component.html",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    CommonModule,
    FormsModule,
    NbAlertModule,
    NbBadgeModule,
    NbButtonModule,
    NbCardModule,
    NbDialogModule,
    NbFormFieldModule,
    NbIconModule,
    NbInputModule,
    NbListModule,
    NbOptionModule,
    NbSelectModule,
    NbSpinnerModule,
    NbTooltipModule,
    Angular2SmartTableModule,
  ],
})
export class LoadManagementComponent implements OnInit, OnDestroy {
  private static readonly sheetExitDurationMs = 180;

  private readonly document = inject(DOCUMENT);
  private readonly clusterContext = inject(ClusterContextService);
  private readonly loadService = inject(LoadService);
  private readonly nodeService = inject(NodeService);
  private readonly dialogService = inject(NbDialogService);
  private readonly toastrService = inject(NbToastrService);
  private readonly route = inject(ActivatedRoute);
  private readonly router = inject(Router);
  private readonly cdr = inject(ChangeDetectorRef);
  private readonly destroy$ = new Subject<void>();
  private detailDialogRef?: NbDialogRef<unknown>;
  private detailTrigger?: HTMLElement;
  private readonly detailRequest$ = new Subject<void>();
  private sheetClosing = false;

  @ViewChild("detailDialog") private detailDialog?: TemplateRef<unknown>;

  source = new LocalDataSource();
  activeCluster: Cluster | null = null;
  databases: string[] = [];
  jobs: LoadJob[] = [];
  selectedJob: LoadJob | null = null;
  loading = false;
  errorMessage = "";
  lastUpdated: Date | null = null;
  detailLoading = false;

  filters: {
    db: string;
    type: string;
    state: string;
    search: string;
    range: "24h" | "7d" | "30d" | "all";
  } = {
    db: "",
    type: "",
    state: "",
    search: "",
    range: "24h",
  };

  summary = {
    running: 0,
    queued: 0,
    failed: 0,
    finished: 0,
  };

  settings = {
    mode: "external",
    hideSubHeader: true,
    noDataMessage: "暂无导入任务",
    actions: false,
    selectMode: "single",
    pager: {
      display: true,
      perPage: 20,
    },
    columns: {
      task: {
        title: "任务",
        type: "string",
        width: "20%",
      },
      target: {
        title: "数据库 / 目标表",
        type: "string",
        width: "18%",
      },
      loadType: {
        title: "类型",
        type: "string",
        width: "10%",
      },
      state: {
        title: "状态",
        type: "html",
        width: "10%",
        sanitizer: { bypassHtml: true },
        valuePrepareFunction: (value: string) => this.renderStateBadge(value),
      },
      stage: {
        title: "阶段",
        type: "string",
        width: "13%",
      },
      dataVolume: {
        title: "数据量",
        type: "string",
        width: "10%",
      },
      filtered: {
        title: "过滤",
        type: "string",
        width: "8%",
      },
      createdAt: {
        title: "创建时间",
        type: "string",
        width: "11%",
      },
    },
  };

  ngOnInit(): void {
    const db = this.route.snapshot.queryParamMap.get("db");
    if (db) {
      this.filters.db = db;
    }

    this.clusterContext.activeCluster$
      .pipe(takeUntil(this.destroy$))
      .subscribe((cluster) => {
        this.activeCluster = cluster;
        if (!cluster) {
          this.jobs = [];
          this.selectedJob = null;
          this.summary = { running: 0, queued: 0, failed: 0, finished: 0 };
          void assignTableRows(this.source, []);
          this.cdr.markForCheck();
          return;
        }
        this.loadDatabases();
        this.loadJobs();
      });
  }

  ngOnDestroy(): void {
    this.detailRequest$.next();
    this.detailRequest$.complete();
    this.destroy$.next();
    this.destroy$.complete();
    this.detailDialogRef?.close();
  }

  loadDatabases(): void {
    this.nodeService
      .getDatabases()
      .pipe(take(1), timeout(20_000))
      .subscribe({
        next: (databases) => {
          this.databases = databases.filter(
            (name) => !["information_schema", "_statistics_"].includes(name),
          );
          this.cdr.markForCheck();
        },
        error: () => {
          this.databases = [];
          this.cdr.markForCheck();
        },
      });
  }

  loadJobs(): void {
    if (!this.activeCluster) {
      return;
    }
    this.loading = true;
    this.errorMessage = "";
    this.cdr.markForCheck();

    this.loadService
      .list({
        db: this.filters.db || undefined,
        type: this.filters.type || undefined,
        state: this.filters.state || undefined,
        search: this.filters.search || undefined,
        range: this.filters.range,
        limit: 200,
      })
      .pipe(
        take(1),
        timeout(20_000),
        finalize(() => {
          this.loading = false;
          this.cdr.markForCheck();
        }),
      )
      .subscribe({
        next: (response) => {
          this.jobs = this.sortJobs(response.items);
          this.summary = response.summary;
          this.lastUpdated = new Date();
          if (this.selectedJob) {
            const selectedKey = this.jobKey(this.selectedJob);
            this.selectedJob =
              this.jobs.find((job) => this.jobKey(job) === selectedKey) || null;
          }
          void assignTableRows(
            this.source,
            this.jobs.map((job) => this.toTableRow(job)),
          );
          this.cdr.markForCheck();
        },
        error: (error) => {
          this.errorMessage = ErrorHandler.handleClusterError(error);
          this.jobs = [];
          this.selectedJob = null;
          this.summary = { running: 0, queued: 0, failed: 0, finished: 0 };
          void assignTableRows(this.source, []);
          this.cdr.markForCheck();
        },
      });
  }

  applyFilters(): void {
    this.syncFiltersToUrl();
    this.loadJobs();
  }

  onSearchKeydown(event: KeyboardEvent): void {
    if (event.key === "Enter") {
      this.applyFilters();
    }
  }

  onRowSelect(event: RowSelectionEvent): void {
    const row = event.data as LoadTableRow | null;
    if (row?.job) {
      this.openDetails(row.job, event.row?.index);
    }
  }

  openDetails(job: LoadJob, rowIndex?: number): void {
    const template = this.detailDialog;
    if (!template || this.sheetClosing) {
      return;
    }
    this.captureDetailTrigger(rowIndex);
    this.selectedJob = job;
    const dialogRef = this.dialogService.open(template, {
      autoFocus: false,
      backdropClass: "side-sheet-backdrop",
      closeOnBackdropClick: false,
      closeOnEsc: false,
      dialogClass: "side-sheet",
      hasBackdrop: true,
      hasScroll: true,
    });
    this.document.defaultView?.requestAnimationFrame(() => {
      this.document
        .querySelector<HTMLElement>(
          ".cdk-overlay-pane.side-sheet .load-detail-sheet",
        )
        ?.focus({ preventScroll: true });
    });
    this.detailDialogRef = dialogRef;
    this.loadJobDetails(job);
    dialogRef.onBackdropClick.pipe(take(1)).subscribe(() => {
      this.closeDetails(dialogRef);
    });
    dialogRef.onClose.pipe(take(1)).subscribe(() => {
      this.detailRequest$.next();
      this.detailLoading = false;
      this.selectedJob = null;
      this.detailDialogRef = undefined;
      this.sheetClosing = false;
      this.restoreDetailFocus();
      this.cdr.markForCheck();
    });
  }

  closeDetails(ref?: NbDialogRef<unknown>): void {
    const dialogRef = ref || this.detailDialogRef;
    if (!dialogRef || this.sheetClosing) {
      return;
    }

    const sheet = this.document.querySelector<HTMLElement>(
      ".cdk-overlay-pane.side-sheet",
    );
    if (!sheet || this.prefersReducedMotion()) {
      dialogRef.close();
      return;
    }

    this.sheetClosing = true;
    sheet.classList.add("side-sheet--closing");
    this.document
      .querySelector<HTMLElement>(".cdk-overlay-backdrop.side-sheet-backdrop")
      ?.classList.add("side-sheet-backdrop--closing");
    this.document.defaultView?.setTimeout(
      () => dialogRef.close(),
      LoadManagementComponent.sheetExitDurationMs,
    );
  }

  jobKey(job: LoadJob): string {
    return job.job_id || job.label || "";
  }

  statusLabel(state: string): string {
    const labels: Record<string, string> = {
      pending: "待处理",
      begin: "开始",
      queueing: "排队中",
      before_load: "准备中",
      loading: "导入中",
      preparing: "准备提交",
      prepared: "待提交",
      committed: "已提交",
      commited: "已提交",
      finished: "已完成",
      cancelled: "已取消",
      canceled: "已取消",
      failed: "失败",
    };
    return labels[state.toLowerCase()] || state || "未知";
  }

  badgeStatus(
    state: string,
  ): "success" | "info" | "warning" | "danger" | "basic" {
    const value = state.toLowerCase();
    if (value.includes("cancel")) return "basic";
    if (value.includes("fail") || value.includes("error")) return "danger";
    if (
      ["finished", "committed", "commited", "success", "succeed"].includes(
        value,
      )
    )
      return "success";
    if (["pending", "queueing", "before_load", "queued"].includes(value))
      return "warning";
    return "info";
  }

  rangeLabel(): string {
    const labels: Record<typeof this.filters.range, string> = {
      "24h": "最近 24 小时",
      "7d": "最近 7 天",
      "30d": "最近 30 天",
      all: "全部时间",
    };
    return labels[this.filters.range];
  }

  summaryMessage(): string {
    if (this.summary.failed > 0) {
      const active = this.activeSummary();
      return active
        ? `${this.summary.failed} 个导入任务失败，请优先排查失败任务；${active}`
        : `${this.summary.failed} 个导入任务失败，请优先排查失败任务`;
    }
    const active = this.summary.running + this.summary.queued;
    if (active > 0) {
      return `${active} 个导入任务仍在处理：${this.activeSummary()}`;
    }
    if (this.summary.finished > 0) {
      return `当前没有待处理任务，${this.summary.finished} 个任务已完成`;
    }
    return "当前筛选范围内没有导入任务";
  }

  private activeSummary(): string {
    return [
      this.summary.running > 0 ? `${this.summary.running} 个运行中` : "",
      this.summary.queued > 0 ? `${this.summary.queued} 个排队中` : "",
    ]
      .filter(Boolean)
      .join("，");
  }

  private captureDetailTrigger(rowIndex?: number): void {
    const rows = this.document.querySelectorAll<HTMLElement>(
      "angular2-smart-table tbody tr",
    );
    const visibleRowIndex =
      rowIndex === undefined
        ? undefined
        : rowIndex % this.settings.pager.perPage;
    const row =
      visibleRowIndex === undefined ? undefined : rows.item(visibleRowIndex);
    if (row) {
      row.tabIndex = -1;
      row.focus();
    }
    const activeElement = this.document.activeElement;
    this.detailTrigger =
      activeElement instanceof HTMLElement ? activeElement : undefined;
  }

  private restoreDetailFocus(): void {
    const trigger = this.detailTrigger;
    this.detailTrigger = undefined;
    if (trigger?.isConnected) {
      this.document.defaultView?.setTimeout(() => trigger.focus());
    }
  }

  private prefersReducedMotion(): boolean {
    return (
      this.document.defaultView?.matchMedia("(prefers-reduced-motion: reduce)")
        .matches || false
    );
  }

  private renderStateBadge(state: string): string {
    return `<span class="badge badge-${this.badgeStatus(state)}">${this.escapeHtml(this.statusLabel(state))}</span>`;
  }

  typeLabel(type: string): string {
    const labels: Record<string, string> = {
      stream_load: "Stream Load",
      routine_load: "Routine Load",
      broker_load: "Broker Load",
      spark_load: "Spark Load",
      insert: "INSERT",
    };
    return labels[type.toLowerCase()] || type || "未知";
  }

  formatNumber(value?: number): string {
    return value === undefined || value === null
      ? "-"
      : new Intl.NumberFormat("zh-CN").format(value);
  }

  formatBytes(value?: number): string {
    if (value === undefined || value === null) return "-";
    if (value < 1024) return `${this.formatNumber(value)} B`;
    const units = ["KB", "MB", "GB", "TB"];
    let amount = value;
    let unit = "B";
    for (const nextUnit of units) {
      amount /= 1024;
      unit = nextUnit;
      if (amount < 1024) break;
    }
    return `${amount.toFixed(amount >= 10 ? 0 : 1)} ${unit}`;
  }

  formatDuration(job: LoadJob): string {
    const duration = job.stage_timeline.reduce(
      (total, stage) => total + stage.duration_ms,
      0,
    );
    if (!duration) return "-";
    if (duration < 1000) return `${duration} ms`;
    const seconds = Math.round(duration / 1000);
    if (seconds < 60) return `${seconds}s`;
    return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
  }

  copyToClipboard(value: string, label: string): void {
    if (!value) return;
    const done = () => this.toastrService.success(`${label}已复制`, "成功");
    const failed = () =>
      this.toastrService.danger("复制失败，请手动选择文本", "失败");

    if (navigator.clipboard?.writeText) {
      navigator.clipboard
        .writeText(value)
        .then(done)
        .catch(() => this.legacyCopy(value, done, failed));
    } else {
      this.legacyCopy(value, done, failed);
    }
  }

  qualityRate(job: LoadJob): string {
    if (job.filtered_rows === undefined || job.scan_rows === undefined)
      return "-";
    if (job.scan_rows <= 0) return "0%";
    const rate = (job.filtered_rows / job.scan_rows) * 100;
    return `${rate.toFixed(rate >= 10 ? 0 : 1)}%`;
  }

  prettyJson(value?: string): string {
    if (!value) return "";
    try {
      return JSON.stringify(JSON.parse(value), null, 2);
    } catch {
      return value;
    }
  }

  private toTableRow(job: LoadJob): LoadTableRow {
    const lastStage = job.stage_timeline[job.stage_timeline.length - 1];
    return {
      task: job.label || job.job_id || "未命名任务",
      target: `${job.database || "-"} / ${job.table_name || "-"}`,
      loadType: this.typeLabel(job.load_type),
      state: job.state,
      stage: lastStage
        ? `${lastStage.label} · ${this.formatDuration(job)}`
        : "引擎未提供阶段时间",
      dataVolume: `${this.formatNumber(job.sink_rows ?? job.scan_rows)} 行 / ${this.formatBytes(job.scan_bytes)}`,
      filtered: `${this.qualityRate(job)} / ${this.formatNumber(job.filtered_rows)} 行`,
      createdAt: job.create_time || "-",
      job,
    };
  }

  private escapeHtml(value: string): string {
    return value.replace(/[&<>'"]/g, (char) => {
      switch (char) {
        case "&":
          return "&amp;";
        case "<":
          return "&lt;";
        case ">":
          return "&gt;";
        case "'":
          return "&#39;";
        case '"':
          return "&quot;";
        default:
          return char;
      }
    });
  }

  private legacyCopy(
    value: string,
    done: () => void,
    failed: () => void,
  ): void {
    const textarea = document.createElement("textarea");
    textarea.value = value;
    textarea.setAttribute("readonly", "");
    textarea.style.position = "fixed";
    textarea.style.opacity = "0";
    document.body.appendChild(textarea);
    textarea.select();
    try {
      document.execCommand("copy") ? done() : failed();
    } catch {
      failed();
    } finally {
      document.body.removeChild(textarea);
    }
  }

  private sortJobs(jobs: LoadJob[]): LoadJob[] {
    const priority = (state: string): number => {
      const value = state.toLowerCase();
      if (value.includes("fail") || value.includes("error")) return 0;
      if (
        ["loading", "preparing", "prepared", "begin", "before_load"].includes(
          value,
        )
      )
        return 1;
      if (["pending", "queueing", "queued"].includes(value)) return 2;
      if (value.includes("cancel")) return 3;
      return 4;
    };

    return [...jobs].sort(
      (left, right) =>
        priority(left.state) - priority(right.state) ||
        (right.create_time || "").localeCompare(left.create_time || ""),
    );
  }

  private loadJobDetails(job: LoadJob): void {
    if (!job.job_id) {
      return;
    }
    this.detailRequest$.next();
    this.detailLoading = true;
    this.loadService
      .get(job.job_id, job.database)
      .pipe(
        take(1),
        takeUntil(this.detailRequest$),
        timeout(20_000),
        finalize(() => {
          this.detailLoading = false;
          this.cdr.markForCheck();
        }),
      )
      .subscribe({
        next: (details) => {
          if (this.detailDialogRef && this.jobKey(this.selectedJob || job) === this.jobKey(job)) {
            this.selectedJob = details;
          }
        },
        error: () => {
          // 详情查询失败时保留列表快照，避免遮挡用户已看到的失败信息。
        },
      });
  }

  private syncFiltersToUrl(): void {
    void this.router.navigate([], {
      relativeTo: this.route,
      queryParams: {
        ...(this.filters.db ? { db: this.filters.db } : {}),
        ...(this.filters.type ? { type: this.filters.type } : {}),
        ...(this.filters.state ? { state: this.filters.state } : {}),
        ...(this.filters.search ? { search: this.filters.search } : {}),
        ...(this.filters.range !== "24h" ? { range: this.filters.range } : {}),
      },
      replaceUrl: true,
    });
  }
}
