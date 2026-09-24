import { TestBed } from "@angular/core/testing";
import { of } from "rxjs";

import { ApiService } from "../api.service";
import { NodeService } from "../node.service";

describe("NodeService", () => {
  const apiService = {
    post: jasmine.createSpy("post").and.returnValue(of({ success: true })),
    get: jasmine.createSpy("get").and.returnValue(of({})),
    getBlob: jasmine.createSpy("getBlob").and.returnValue(of(new Blob())),
  };
  let service: NodeService;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [NodeService, { provide: ApiService, useValue: apiService }],
    });
    service = TestBed.inject(NodeService);
    apiService.post.calls.reset();
    apiService.get.calls.reset();
    apiService.getBlob.calls.reset();
  });

  it("posts local files to the dedicated Stream Load endpoint", () => {
    const formData = new FormData();

    service.streamLoad(formData).subscribe();

    expect(apiService.post).toHaveBeenCalledWith(
      "/clusters/queries/stream-load",
      formData,
      910000,
    );
  });

  it("loads FE nodes for the explicitly selected cluster", () => {
    service.listFrontends(7).subscribe();

    expect(apiService.get).toHaveBeenCalledWith("/clusters/frontends", { cluster_id: 7 });
  });

  it("uses the isolated FE profile APIs with discovered identity", () => {
    const frontend = {
      cluster_id: 7,
      name: "fe_1",
      host: "10.0.0.8",
      http_port: "8030",
    };

    service.listFrontendProfiles(frontend).subscribe();
    expect(apiService.get).toHaveBeenCalledWith(
      "/clusters/frontends/profiles",
      frontend,
    );

    service.getFrontendProfile(frontend, "mem-profile-20260918-123456.html.tar.gz").subscribe();
    expect(apiService.getBlob).toHaveBeenCalledWith(
      "/clusters/frontends/profiles/file",
      { ...frontend, filename: "mem-profile-20260918-123456.html.tar.gz" },
    );
  });

  it("uses the fixed backend diagnostic API with discovered node identity", () => {
    const backend = {
      cluster_id: 7,
      backend_id: "42",
      host: "10.0.0.9",
      heartbeat_port: "9050",
      http_port: "8040",
      include: "blocking_drivers" as const,
    };

    service.getBackendDiagnostics(backend).subscribe();

    expect(apiService.get).toHaveBeenCalledWith("/clusters/backends/diagnostics", backend);
  });

  it("requests the profile retest series by query id", () => {
    service.getProfileRetest("abc-123").subscribe();

    expect(apiService.get).toHaveBeenCalledWith(
      "/clusters/profiles/abc-123/retest",
    );
  });
});
