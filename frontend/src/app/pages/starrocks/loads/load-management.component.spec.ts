import { ChangeDetectorRef, DOCUMENT, TemplateRef } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { ActivatedRoute, Router, convertToParamMap } from "@angular/router";
import { NbDialogService, NbToastrService } from "@nebular/theme";
import { RowSelectionEvent } from "angular2-smart-table";
import { Subject, of } from "rxjs";

import { ClusterContextService } from "../../../@core/data/cluster-context.service";
import { LoadJob, LoadService } from "../../../@core/data/load.service";
import { NodeService } from "../../../@core/data/node.service";
import { LoadManagementComponent } from "./load-management.component";

describe("LoadManagementComponent", () => {
  let component: LoadManagementComponent;
  const dialogService = { open: jasmine.createSpy("open") };
  const loadService = {
    get: jasmine.createSpy("get").and.returnValue(of({})),
    list: jasmine.createSpy("list"),
    executeAction: jasmine.createSpy("executeAction"),
  };

  const nodeService = {
    getDatabases: jasmine.createSpy("getDatabases").and.returnValue(of([])),
    getTables: jasmine.createSpy("getTables").and.returnValue(of([])),
    executeSQL: jasmine.createSpy("executeSQL"),
    streamLoad: jasmine.createSpy("streamLoad"),
  };
  const toastrService = {
    show: () => undefined,
    success: jasmine.createSpy("success"),
  };

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        {
          provide: ClusterContextService,
          useValue: { activeCluster$: of(null) },
        },
        { provide: LoadService, useValue: loadService },
        { provide: NodeService, useValue: nodeService },
        { provide: NbDialogService, useValue: dialogService },
        { provide: NbToastrService, useValue: toastrService },
        {
          provide: ActivatedRoute,
          useValue: { snapshot: { queryParamMap: convertToParamMap({}) } },
        },
        {
          provide: Router,
          useValue: { navigate: () => Promise.resolve(true) },
        },
        {
          provide: ChangeDetectorRef,
          useValue: { markForCheck: () => undefined },
        },
        { provide: DOCUMENT, useValue: document },
      ],
    });
    component = TestBed.runInInjectionContext(
      () => new LoadManagementComponent(),
    );
    dialogService.open.calls.reset();
    loadService.get.calls.reset();
    loadService.list.calls.reset();
    loadService.executeAction.calls.reset();
    nodeService.getDatabases.calls.reset();
    nodeService.getTables.calls.reset();
    nodeService.getTables.and.returnValue(of([]));
    nodeService.executeSQL.calls.reset();
    nodeService.streamLoad.calls.reset();
    toastrService.success.calls.reset();
  });

  it("uses CN terminology for shared-data routine-load tasks", () => {
    component.activeCluster = {
      deployment_mode: "shared_data",
    } as NonNullable<typeof component.activeCluster>;

    expect(component.computeNodeLabel).toBe("CN");
  });

  it("prioritizes failed tasks in the situation summary", () => {
    component.summary = { running: 1, queued: 1, failed: 2, finished: 0 };

    expect(component.summaryMessage()).toBe(
      "2 个导入任务失败，请优先排查失败任务；1 个运行中，1 个排队中",
    );
  });

  it("reports active tasks without turning them into an alert", () => {
    component.summary = { running: 2, queued: 3, failed: 0, finished: 0 };

    expect(component.summaryMessage()).toBe(
      "5 个导入任务仍在处理：2 个运行中，3 个排队中",
    );
  });

  it("keeps the completed state quiet", () => {
    component.summary = { running: 0, queued: 0, failed: 0, finished: 200 };

    expect(component.summaryMessage()).toBe(
      "当前没有待处理任务，200 个任务已完成",
    );
  });

  it("scopes the summary to fetched rows while more cursor pages remain", () => {
    component.nextCursor = "2026-01-01 00:00:00:2";
    component.summary = { running: 0, queued: 0, failed: 0, finished: 200 };

    expect(component.summaryMessage()).toBe(
      "已加载任务中，当前没有待处理任务，200 个任务已完成",
    );
  });

  it("loads the next cursor page without inventing numbered pages", () => {
    component.activeCluster = { cluster_type: "starrocks" } as NonNullable<
      typeof component.activeCluster
    >;
    component.jobs = [
      {
        job_id: "2",
        state: "FINISHED",
        load_type: "INSERT",
        stage_timeline: [],
      },
    ];
    component.nextCursor = "2026-01-01 00:00:00:2";
    loadService.list.and.returnValue(
      of({
        items: [
          {
            job_id: "1",
            state: "LOADING",
            load_type: "STREAM_LOAD",
            stage_timeline: [],
          },
        ],
        summary: { running: 1, queued: 0, failed: 0, finished: 0 },
        total: 1,
        has_more: false,
        source: "information_schema.loads",
      }),
    );

    component.loadMoreJobs();

    expect(loadService.list).toHaveBeenCalledWith(
      jasmine.objectContaining({ cursor: "2026-01-01 00:00:00:2" }),
    );
    expect(component.jobs.map((job) => job.job_id)).toEqual(["1", "2"]);
    expect(component.summary).toEqual({
      running: 1,
      queued: 0,
      failed: 0,
      finished: 1,
    });
    expect(component.settings.pager).toEqual({
      display: false,
      perPage: Number.MAX_SAFE_INTEGER,
    });
  });

  it("opens the single import dialog once as a side sheet", () => {
    dialogService.open.and.returnValue({
      close: jasmine.createSpy("close"),
      onClose: of(undefined),
    });
    const template = {} as TemplateRef<unknown>;
    (
      component as unknown as { importDialog: TemplateRef<unknown> }
    ).importDialog = template;
    component.activeCluster = { cluster_type: "starrocks" } as NonNullable<
      typeof component.activeCluster
    >;

    component.openImportDialog();

    expect(dialogService.open).toHaveBeenCalledTimes(1);
    expect(dialogService.open).toHaveBeenCalledWith(
      template,
      jasmine.objectContaining({ dialogClass: "side-sheet" }),
    );
  });

  it("does not open the import dialog for Doris", () => {
    (
      component as unknown as { importDialog: TemplateRef<unknown> }
    ).importDialog = {} as TemplateRef<unknown>;
    component.activeCluster = { cluster_type: "doris" } as NonNullable<
      typeof component.activeCluster
    >;

    component.openImportDialog();

    expect(dialogService.open).not.toHaveBeenCalled();
    expect(nodeService.getTables).not.toHaveBeenCalled();
  });

  it("switches the source type inside the same dialog", () => {
    component.importForm.type = "path";
    component.importForm.format = "parquet";

    component.onImportTypeChange("kafka");

    expect(component.importForm.type).toBe("kafka");
    expect(component.importForm.format).toBe("csv");

    component.onImportTypeChange("path");

    expect(component.importForm.label).toMatch(/^stellar_broker_\d+$/);
  });

  it("submits local files through the dedicated Stream Load path", () => {
    component.importForm.type = "file";
    component.importForm.database = "target_db";
    component.importForm.table = "target_table";
    component.importForm.format = "csv";
    component.importForm.label = "daily_load";
    component.importForm.columnSeparator = "\\t";
    component.importForm.file = new File(["id\\n1\\n"], "daily.csv", {
      type: "text/csv",
    });
    const dialogRef = { close: jasmine.createSpy("close") };
    nodeService.streamLoad.and.returnValue(
      of({ success: true, number_loaded_rows: 1 }),
    );
    loadService.list.and.returnValue(
      of({
        items: [],
        summary: { running: 0, queued: 0, failed: 0, finished: 0 },
      }),
    );

    component.submitImport(dialogRef as unknown as any);

    const formData = nodeService.streamLoad.calls.mostRecent()
      .args[0] as FormData;
    expect(formData.get("database")).toBe("target_db");
    expect(formData.get("table")).toBe("target_table");
    expect(formData.get("format")).toBe("csv");
    expect(formData.get("label")).toBe("daily_load");
    expect(formData.get("column_separator")).toBe("\\t");
    expect(formData.get("file")).toEqual(jasmine.any(File));
    expect(dialogRef.close).toHaveBeenCalled();
  });

  it("derives JSON format from a selected JSON file", () => {
    component.importForm.format = "csv";
    component.onImportFileChange({
      target: { files: [new File(["{}"], "events.json")] },
    } as unknown as Event);

    expect(component.importForm.format).toBe("json");
    expect(component.isImportFormValid()).toBeFalse();
  });

  it("builds a credential-free Broker Load statement", () => {
    component.importForm = {
      ...component.importForm,
      type: "path",
      database: "analytics",
      table: "events",
      path: "hdfs://namenode:8020/data/events/*.parquet",
      format: "parquet",
      label: "events_backfill",
    };

    expect(component.buildImportSql()).toBe(
      "LOAD LABEL `analytics`.`events_backfill` (\n" +
        '  DATA INFILE ("hdfs://namenode:8020/data/events/*.parquet") INTO TABLE `events`\n' +
        '  FORMAT AS "PARQUET"\n' +
        ")\n" +
        "WITH BROKER;",
    );
  });

  it("rejects credentials, query parameters and root-only Broker Load paths", () => {
    component.importForm = {
      ...component.importForm,
      type: "path",
      database: "analytics",
      table: "events",
      format: "csv",
      label: "events_backfill",
    };

    for (const path of [
      "hdfs://reader:secret@namenode:8020/data/events.csv",
      "hdfs://namenode:8020/data/events.csv?password=secret",
      'hdfs://namenode:8020/data/"events.csv',
      "hdfs://namenode:8020",
    ]) {
      component.importForm.path = path;
      expect(component.isImportFormValid()).toBeFalse();
    }

    expect(component.buildImportSql()).toBe("");
  });

  it("submits a Broker Load without recording the source path in SQL history", () => {
    component.importForm = {
      ...component.importForm,
      type: "path",
      database: "analytics",
      table: "events",
      path: "file:///mnt/nas/events/*.csv",
      format: "csv",
      label: "events_backfill",
    };
    const dialogRef = { close: jasmine.createSpy("close") };
    nodeService.executeSQL.and.returnValue(
      of({ results: [{ success: true }] }),
    );
    loadService.list.and.returnValue(
      of({
        items: [],
        summary: { running: 0, queued: 0, failed: 0, finished: 0 },
      }),
    );

    component.submitImport(dialogRef as unknown as any);

    expect(nodeService.executeSQL).toHaveBeenCalledWith(
      component.buildImportSql(),
      undefined,
      undefined,
      "analytics",
    );
    expect(dialogRef.close).toHaveBeenCalled();
  });

  it("builds a plaintext Routine Load statement and rejects unsafe Kafka values", () => {
    component.importForm = {
      ...component.importForm,
      type: "kafka",
      database: "analytics",
      table: "events",
      jobName: "events_topic",
      brokers: "kafka-1.example:9092,[2001:db8::1]:9093",
      topic: "events",
      format: "json",
      offset: "OFFSET_END",
    };

    expect(component.buildImportSql()).toContain(
      "CREATE ROUTINE LOAD `analytics`.`events_topic` ON `events`",
    );
    expect(component.buildImportSql()).toContain(
      '"property.kafka_default_offsets" = "OFFSET_END"',
    );
    component.importForm.brokers = "reader:secret@kafka-1.example:9092";
    expect(component.isImportFormValid()).toBeFalse();
    component.importForm.brokers = "kafka-1.example:9092";
    component.importForm.topic = 'events"; DROP TABLE events';
    expect(component.isImportFormValid()).toBeFalse();
  });

  it("keeps real stage durations and only bars what the engine reports", () => {
    const timed = {
      job_id: "j1",
      state: "FINISHED",
      load_type: "BROKER_LOAD",
      scan_rows: 1000,
      filtered_rows: 30,
      create_time: "2026-09-21 11:30:03",
      stage_timeline: [
        {
          key: "pending",
          label: "排队",
          duration_ms: 1000,
          status: "completed",
        },
        {
          key: "loading",
          label: "导入",
          duration_ms: 9000,
          status: "completed",
        },
      ],
    } as LoadJob;
    const untimed = {
      job_id: "j2",
      state: "FINISHED",
      load_type: "STREAM_LOAD",
      stage_timeline: [],
    } as LoadJob;
    (component as unknown as { maxDurationMs: number }).maxDurationMs = 10_000;
    const rows = [timed, untimed].map((job) =>
      (
        component as unknown as {
          toTableRow: (job: LoadJob) => any;
        }
      ).toTableRow(job),
    );

    expect(rows[0].stage).toContain("导入");
    expect(rows[0].stage).toContain("10s");
    expect(rows[0].stage).toContain("width:100%");
    expect(rows[0].createdAt).toContain("11:30:03");
    expect(rows[0].createdAt).toContain("09-21");
    expect(rows[0].filtered).toContain("cell-warn");
    expect(rows[1].stage).toBe('<span class="cell-clip">-</span>');
    expect(rows[1].stage).not.toContain("cell-bar");
    expect(rows[1].filtered).toBe('<span class="cell-clip">-</span>');
    expect(rows[1].filtered).not.toContain("cell-warn");
  });

  it("executes an engine action and reloads the job detail", () => {
    const job = {
      job_id: "42",
      database: "analytics",
      state: "PAUSED",
      load_type: "ROUTINE_LOAD",
    } as LoadJob;
    component.selectedJob = job;
    (component as unknown as { detailDialogRef?: unknown }).detailDialogRef =
      {} as never;
    loadService.executeAction.and.returnValue(
      of({ success: true, message: "恢复作业已下发", state: "RUNNING" }),
    );
    loadService.get.and.returnValue(of({ ...job, state: "RUNNING" }));

    component.executeLoadAction({
      action: "resume_routine",
      label: "恢复作业",
      description: "父作业当前为 PAUSED",
      statement: "RESUME ROUTINE LOAD FOR `analytics`.`events_topic_load`",
    });

    expect(loadService.executeAction).toHaveBeenCalledWith(
      "42",
      "resume_routine",
    );
    expect(loadService.get).toHaveBeenCalledWith("42", "analytics");
    expect(component.selectedJob?.state).toBe("RUNNING");
  });

  it("prefills the import drawer from a failed job context", () => {
    component.activeCluster = { cluster_type: "starrocks" } as NonNullable<
      typeof component.activeCluster
    >;
    (
      component as unknown as { importDialog: TemplateRef<unknown> }
    ).importDialog = {} as TemplateRef<unknown>;
    dialogService.open.and.returnValue({
      close: jasmine.createSpy("close"),
      onClose: of(undefined),
    });
    component.databases = ["analytics"];

    component.openImportDialog({
      type: "path",
      database: "analytics",
      table: "events",
    });

    expect(component.importForm.type).toBe("path");
    expect(component.importForm.database).toBe("analytics");
    expect(component.importForm.table).toBe("events");
    // Broker 类型需要默认 Label，且源路径不会被预填
    expect(component.importForm.label).toMatch(/^stellar_broker_\d+$/);
    expect(component.importForm.path).toBe("");
  });

  it("offers re-import only for failed or cancelled jobs", () => {
    component.activeCluster = { cluster_type: "starrocks" } as NonNullable<
      typeof component.activeCluster
    >;

    expect(
      component.canReopenImport({ state: "CANCELLED" } as LoadJob),
    ).toBeTrue();
    expect(
      component.canReopenImport({ state: "FAILED" } as LoadJob),
    ).toBeTrue();
    expect(
      component.canReopenImport({ state: "FINISHED" } as LoadJob),
    ).toBeFalse();
    expect(
      component.canReopenImport({ state: "LOADING" } as LoadJob),
    ).toBeFalse();

    component.activeCluster = { cluster_type: "doris" } as NonNullable<
      typeof component.activeCluster
    >;
    expect(
      component.canReopenImport({ state: "CANCELLED" } as LoadJob),
    ).toBeFalse();
  });

  it("passes the selected Smart Table row index to the Sheet opener", () => {
    const job = { job_id: "job-1", state: "FINISHED" } as LoadJob;
    const openDetails = spyOn(component, "openDetails");

    component.onRowSelect({
      data: { job },
      row: { index: 4 },
    } as unknown as RowSelectionEvent);

    expect(openDetails).toHaveBeenCalledWith(job, 4);
  });

  it("focuses the selected row without local table pagination", () => {
    const table = document.createElement("angular2-smart-table");
    table.innerHTML = "<table><tbody><tr></tr><tr></tr></tbody></table>";
    document.body.append(table);

    (
      component as unknown as {
        captureDetailTrigger: (rowIndex: number) => void;
      }
    ).captureDetailTrigger(1);

    expect(document.activeElement).toBe(
      table.querySelectorAll("tr").item(1),
    );
    table.remove();
  });

  it("does not autofocus the Sheet close button", () => {
    const onBackdropClick = new Subject<void>();
    const onClose = new Subject<void>();
    dialogService.open.and.returnValue({
      close: jasmine.createSpy("close"),
      onBackdropClick,
      onClose,
    });
    const template = {} as TemplateRef<unknown>;
    (
      component as unknown as { detailDialog: TemplateRef<unknown> }
    ).detailDialog = template;

    component.openDetails({ job_id: "job-1", state: "FINISHED" } as LoadJob);

    expect(dialogService.open).toHaveBeenCalledWith(
      template,
      jasmine.objectContaining({ autoFocus: false }),
    );
  });

  it("keeps the selected Doris failure fields in the detail response", () => {
    const onBackdropClick = new Subject<void>();
    const onClose = new Subject<void>();
    dialogService.open.and.returnValue({
      close: jasmine.createSpy("close"),
      onBackdropClick,
      onClose,
    });
    loadService.get.and.returnValue(
      of({
        job_id: "42",
        state: "CANCELLED",
        load_type: "BROKER_LOAD",
        stage_timeline: [],
        doris_failure: {
          url: "https://errors/42",
          error_msg: "selected failure",
          job_details: '{"id":42}',
        },
      } as LoadJob),
    );
    (
      component as unknown as { detailDialog: TemplateRef<unknown> }
    ).detailDialog = {} as TemplateRef<unknown>;

    component.openDetails({ job_id: "42", state: "CANCELLED" } as LoadJob);

    expect(component.selectedJob?.doris_failure?.url).toBe("https://errors/42");
    expect(component.selectedJob?.doris_failure?.job_details).toBe('{"id":42}');
  });
});
