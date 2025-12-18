import { Injectable } from '@angular/core';
import { NbDialogService } from '@nebular/theme';
import { Observable } from 'rxjs';
import { ConfirmDialogComponent } from '../components/confirm-dialog/confirm-dialog.component';

@Injectable({
  providedIn: 'root'
})
export class ConfirmDialogService {
  constructor(private dialogService: NbDialogService) {}

  confirm(
    title: string,
    message: string,
    confirmText: string = '确定',
    cancelText: string = '取消',
    confirmStatus: string = 'primary'
  ): Observable<boolean> {
    return this.dialogService.open(ConfirmDialogComponent, {
      context: {
        title,
        message,
        confirmText,
        cancelText,
        confirmStatus
      },
      hasBackdrop: true,
      closeOnBackdropClick: false,
      closeOnEsc: true,
      autoFocus: true
    }).onClose;
  }

  confirmDelete(itemName: string, additionalWarning?: string): Observable<boolean> {
    const message = additionalWarning 
      ? `确定要删除 "${itemName}" 吗？\n\n${additionalWarning}`
      : `确定要删除 "${itemName}" 吗？`;

    return this.confirm(
      '确认删除',
      message,
      '删除',
      '取消',
      'danger'
    );
  }
}
