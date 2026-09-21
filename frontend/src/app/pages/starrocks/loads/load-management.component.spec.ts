import { ChangeDetectorRef, DOCUMENT, TemplateRef } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { ActivatedRoute, Router, convertToParamMap } from "@angular/router";
import { NbDialogService, NbToastrService } from "@nebular/theme";
import { RowSelectionEvent } from "angular2-smart-table";
import { Subject, of } from "rxjs";

import { ClusterContextService } from "../../../@core/data/cluster-context.service";
import { LoadJob, LoadService } from "../../../@core/data/load.service";
import { NodeService } from "../../../@core/data/node.service";
import { ConfirmDialogService } from "../../../@core/services/confirm-dialog.service";
import { LoadManagementComponent } from "./load-management.component";

describe("LoadManagementComponent", () => {
  let component: LoadManagementComponent;
  const dialogService = { open: jasmine.createSpy("open") };
  const loadService = {
    get: jasmine.createSpy("get").and.returnValue(of({})),
    list: jasmine.createSpy("list"),
  };

  const nodeService = {
    getDatabases: jasmine.createSpy("getDatabases").and.returnValue(of([])),
    getTables: jasmine.createSpy("getTables").and.returnValue(of([])),
    executeSQL: jasmine.createSpy("executeSQL"),
    streamLoad: jasmine.createSpy("streamLoad"),
  };
  const confirmDialogService = {
    confirm: jasmine.createSpy("confirm"),
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
        { provide: ConfirmDialogService, useValue: confirmDialogService },
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
    nodeService.getDatabases.calls.reset();
    nodeService.getTables.calls.reset();
    nodeService.getTables.and.returnValue(of([]));
    nodeService.executeSQL.calls.reset();
    nodeService.streamLoad.calls.reset();
    confirmDialogService.confirm.calls.reset();
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

  it("submits local files through the dedicated Stream Load path", () => {
    component.streamLoadForm = {
      database: "target_db",
      table: "target_table",
      format: "csv",
      label: "daily_load",
      columnSeparator: "\\t",
      file: new File(["id\\n1\\n"], "daily.csv", { type: "text/csv" }),
    };
    const dialogRef = { close: jasmine.createSpy("close") };
    confirmDialogService.confirm.and.returnValue(of(true));
    nodeService.streamLoad.and.returnValue(
      of({ success: true, number_loaded_rows: 1 }),
    );
    loadService.list.and.returnValue(
      of({
        items: [],
        summary: { running: 0, queued: 0, failed: 0, finished: 0 },
      }),
    );

    component.submitStreamLoad(dialogRef as unknown as any);

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

  it("routes the StarRocks import action through the source chooser", () => {
    dialogService.open.and.returnValue({
      close: jasmine.createSpy("close"),
      onClose: of(undefined),
    });
    (
      component as unknown as { importChooserDialog: TemplateRef<unknown> }
    ).importChooserDialog = {} as TemplateRef<unknown>;
    component.activeCluster = { cluster_type: "starrocks" } as NonNullable<
      typeof component.activeCluster
    >;

    component.openImportChooser();

    expect(dialogService.open).toHaveBeenCalled();
  });

  it("does not open an empty import chooser for Doris", () => {
    (
      component as unknown as { importChooserDialog: TemplateRef<unknown> }
    ).importChooserDialog = {} as TemplateRef<unknown>;
    component.activeCluster = { cluster_type: "doris" } as NonNullable<
      typeof component.activeCluster
    >;

    component.openImportChooser();

    expect(dialogService.open).not.toHaveBeenCalled();
  });

  it("does not open local file delivery for Doris", () => {
    (
      component as unknown as { streamLoadDialog: TemplateRef<unknown> }
    ).streamLoadDialog = {} as TemplateRef<unknown>;
    component.activeCluster = { cluster_type: "doris" } as NonNullable<
      typeof component.activeCluster
    >;

    component.openStreamLoad();

    expect(dialogService.open).not.toHaveBeenCalled();
    expect(nodeService.getTables).not.toHaveBeenCalled();
  });

  it("derives JSON format from a selected JSON file", () => {
    component.streamLoadForm.format = "csv";
    component.onStreamFileChange({
      target: { files: [new File(["{}"], "events.json")] },
    } as unknown as Event);

    expect(component.streamLoadForm.format).toBe("json");
    expect(component.isStreamLoadFormValid()).toBeFalse();
  });

  it("builds a credential-free Broker Load statement", () => {
    component.brokerLoadForm = {
      database: "analytics",
      table: "events",
      path: "hdfs://namenode:8020/data/events/*.parquet",
      format: "parquet",
      label: "events_backfill",
      columnSeparator: ",",
    };

    expect(component.buildBrokerLoadSql()).toBe(
      "LOAD LABEL `analytics`.`events_backfill` (\n" +
        '  DATA INFILE ("hdfs://namenode:8020/data/events/*.parquet") INTO TABLE `events`\n' +
        '  FORMAT AS "PARQUET"\n' +
        ")\n" +
        "WITH BROKER;",
    );
  });

  it("rejects credentials embedded in a Broker Load path", () => {
    component.brokerLoadForm = {
      database: "analytics",
      table: "events",
      path: "hdfs://reader:secret@namenode:8020/data/events.csv",
      format: "csv",
      label: "events_backfill",
      columnSeparator: ",",
    };

    expect(component.isBrokerLoadFormValid()).toBeFalse();
    expect(component.buildBrokerLoadSql()).toBe("");
  });

  it("rejects Broker Load path query parameters and quote characters", () => {
    component.brokerLoadForm = {
      database: "analytics",
      table: "events",
      path: "hdfs://namenode:8020/data/events.csv?password=secret",
      format: "csv",
      label: "events_backfill",
      columnSeparator: ",",
    };

    expect(component.isBrokerLoadFormValid()).toBeFalse();
    component.brokerLoadForm.path = 'hdfs://namenode:8020/data/"events.csv';
    expect(component.isBrokerLoadFormValid()).toBeFalse();
    component.brokerLoadForm.path = "hdfs://namenode:8020";
    expect(component.isBrokerLoadFormValid()).toBeFalse();
  });

  it("submits a Broker Load without recording the source path in SQL history", () => {
    component.brokerLoadForm = {
      database: "analytics",
      table: "events",
      path: "file:///mnt/nas/events/*.csv",
      format: "csv",
      label: "events_backfill",
      columnSeparator: ",",
    };
    const dialogRef = { close: jasmine.createSpy("close") };
    confirmDialogService.confirm.and.returnValue(of(true));
    nodeService.executeSQL.and.returnValue(
      of({ results: [{ success: true }] }),
    );
    loadService.list.and.returnValue(
      of({
        items: [],
        summary: { running: 0, queued: 0, failed: 0, finished: 0 },
      }),
    );

    component.submitBrokerLoad(dialogRef as unknown as any);

    expect(nodeService.executeSQL).toHaveBeenCalledWith(
      component.buildBrokerLoadSql(),
      undefined,
      undefined,
      "analytics",
    );
    expect(dialogRef.close).toHaveBeenCalled();
  });

  it("builds a plaintext Routine Load statement and rejects unsafe Kafka values", () => {
    component.routineLoadForm = {
      database: "analytics",
      table: "events",
      jobName: "events_topic",
      brokers: "kafka-1.example:9092,[2001:db8::1]:9093",
      topic: "events",
      format: "json",
      offset: "OFFSET_END",
      columnSeparator: ",",
    };

    expect(component.buildRoutineLoadSql()).toContain(
      "CREATE ROUTINE LOAD `analytics`.`events_topic` ON `events`",
    );
    expect(component.buildRoutineLoadSql()).toContain(
      '"property.kafka_default_offsets" = "OFFSET_END"',
    );
    component.routineLoadForm.brokers = "reader:secret@kafka-1.example:9092";
    expect(component.isRoutineLoadFormValid()).toBeFalse();
    component.routineLoadForm.brokers = "kafka-1.example:9092";
    component.routineLoadForm.topic = 'events"; DROP TABLE events';
    expect(component.isRoutineLoadFormValid()).toBeFalse();
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

  it("focuses the visible row when a paged selection has a global index", () => {
    const table = document.createElement("angular2-smart-table");
    table.innerHTML = "<table><tbody><tr></tr><tr></tr></tbody></table>";
    document.body.append(table);

    (
      component as unknown as {
        captureDetailTrigger: (rowIndex: number) => void;
      }
    ).captureDetailTrigger(20);

    expect(document.activeElement).toBe(table.querySelector("tr"));
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
