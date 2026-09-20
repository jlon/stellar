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
  };

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        {
          provide: ClusterContextService,
          useValue: { activeCluster$: of(null) },
        },
        { provide: LoadService, useValue: loadService },
        { provide: NodeService, useValue: { getDatabases: () => of([]) } },
        { provide: NbDialogService, useValue: dialogService },
        { provide: NbToastrService, useValue: { show: () => undefined } },
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
