import { TestBed } from "@angular/core/testing";
import { of } from "rxjs";

import { ApiService } from "./api.service";
import { NodeService } from "./node.service";

describe("NodeService", () => {
  const apiService = {
    post: jasmine.createSpy("post").and.returnValue(of({ success: true })),
    get: jasmine.createSpy("get").and.returnValue(of({})),
  };
  let service: NodeService;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [NodeService, { provide: ApiService, useValue: apiService }],
    });
    service = TestBed.inject(NodeService);
    apiService.post.calls.reset();
    apiService.get.calls.reset();
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

  it("requests the profile retest series by query id", () => {
    service.getProfileRetest("abc-123").subscribe();

    expect(apiService.get).toHaveBeenCalledWith(
      "/clusters/profiles/abc-123/retest",
    );
  });
});
