import { TestBed } from '@angular/core/testing';
import { NbDialogService } from '@nebular/theme';
import { of } from 'rxjs';

import { ConfirmDialogService } from '../confirm-dialog.service';

describe('ConfirmDialogService', () => {
  const dialogService = { open: jasmine.createSpy('open').and.returnValue({ onClose: of(false) }) };

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        ConfirmDialogService,
        { provide: NbDialogService, useValue: dialogService },
      ],
    });
    dialogService.open.calls.reset();
  });

  it('uses an unblurred secondary backdrop only when nested', () => {
    const service = TestBed.inject(ConfirmDialogService);

    service.confirm('确认', '继续执行吗？', '继续', '取消', 'primary', { nested: true });

    expect(dialogService.open).toHaveBeenCalledWith(
      jasmine.anything(),
      jasmine.objectContaining({
        dialogClass: 'nested-action-dialog',
        backdropClass: 'nested-action-backdrop',
      }),
    );
  });

  it('keeps standalone confirmations free of nested overlay classes', () => {
    const service = TestBed.inject(ConfirmDialogService);

    service.confirm('确认', '继续执行吗？');

    const [, config] = dialogService.open.calls.mostRecent().args;
    expect(config.dialogClass).toBeUndefined();
    expect(config.backdropClass).toBeUndefined();
  });
});
