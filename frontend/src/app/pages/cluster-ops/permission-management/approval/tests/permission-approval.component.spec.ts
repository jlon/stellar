import { TestBed } from '@angular/core/testing';
import { NbDialogService, NbToastrService } from '@nebular/theme';
import { Subject, of } from 'rxjs';

import { I18nService } from '../../../../../@core/i18n/i18n.service';
import { PermissionRequestService } from '../../../../../@core/data/permission-request.service';
import { PermissionRequestResponse } from '../../../../../@core/data/permission-request.model';
import { PermissionApprovalComponent } from '../permission-approval.component';

describe('PermissionApprovalComponent', () => {
  let component: PermissionApprovalComponent;
  const dialogService = { open: jasmine.createSpy('open') };
  const permissionService = {
    approveRequest: jasmine.createSpy('approveRequest').and.returnValue(of(undefined)),
    rejectRequest: jasmine.createSpy('rejectRequest').and.returnValue(of(undefined)),
    listPendingApprovals: jasmine.createSpy('listPendingApprovals').and.returnValue(of([])),
  };
  const toastr = { success: jasmine.createSpy('success'), danger: jasmine.createSpy('danger') };
  const request = { id: 9, applicant_name: 'operator', request_type: 'grant_permission' } as PermissionRequestResponse;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        { provide: NbDialogService, useValue: dialogService },
        { provide: PermissionRequestService, useValue: permissionService },
        { provide: NbToastrService, useValue: toastr },
        { provide: I18nService, useValue: { instant: (value: string) => value } },
      ],
    });
    component = TestBed.runInInjectionContext(() => new PermissionApprovalComponent());
    dialogService.open.calls.reset();
    permissionService.approveRequest.calls.reset();
    permissionService.rejectRequest.calls.reset();
    permissionService.listPendingApprovals.calls.reset();
    toastr.success.calls.reset();
    toastr.danger.calls.reset();
  });

  it('keeps the approval detail open when its nested decision is cancelled', () => {
    const decisionClose$ = new Subject<unknown>();
    const detailRef = {
      close: jasmine.createSpy('close'),
      componentRef: { instance: {} },
    };
    dialogService.open.and.returnValues(detailRef, { onClose: decisionClose$ });

    component.onViewDetail(request);
    (detailRef.componentRef.instance as { onDecision: (approve: boolean) => void }).onDecision(true);

    expect(dialogService.open.calls.mostRecent().args[1]).toEqual(
      jasmine.objectContaining({
        dialogClass: 'nested-action-dialog',
        backdropClass: 'nested-action-backdrop',
        closeOnBackdropClick: false,
        closeOnEsc: true,
      }),
    );
    expect(detailRef.close).not.toHaveBeenCalled();

    decisionClose$.next({ confirmed: false });

    expect(detailRef.close).not.toHaveBeenCalled();
    expect(permissionService.approveRequest).not.toHaveBeenCalled();
  });

  it('closes the approval detail only after a confirmed decision succeeds', () => {
    const detailRef = {
      close: jasmine.createSpy('close'),
      componentRef: { instance: {} },
    };
    dialogService.open.and.returnValues(detailRef, { onClose: of({ confirmed: true, comment: 'approved' }) });

    component.onViewDetail(request);
    (detailRef.componentRef.instance as { onDecision: (approve: boolean) => void }).onDecision(true);

    expect(permissionService.approveRequest).toHaveBeenCalledWith(9, { comment: 'approved' });
    expect(detailRef.close).toHaveBeenCalledTimes(1);
  });
});
