import { Component, Input } from '@angular/core';
import { NbDialogRef } from '@nebular/theme';

/**
 * Confirmation Dialog Component
 * Generic confirmation dialog using ngx-admin native styles
 * Used for approval/rejection confirmations with comment input
 */
@Component({
  selector: 'ngx-confirmation-dialog',
  template: `
    <nb-card class="confirmation-dialog">
      <nb-card-header>
        <h5>{{ title }}</h5>
      </nb-card-header>

      <nb-card-body>
        <div class="message-content" *ngIf="message">
          <p>{{ message }}</p>
        </div>

        <div class="form-group" *ngIf="showCommentInput">
          <label for="comment">
            {{ commentLabel }}
            <span class="text-danger" *ngIf="commentRequired">*</span>
          </label>
          <textarea
            nbInput
            fullWidth
            [rows]="commentRows"
            [(ngModel)]="comment"
            [placeholder]="commentPlaceholder"
            [required]="commentRequired"
            id="comment">
          </textarea>
          <small class="form-text text-muted" *ngIf="commentHint">
            {{ commentHint }}
          </small>
        </div>

        <nb-alert *ngIf="alertMessage" [status]="alertStatus" appearance="outline">
          {{ alertMessage }}
        </nb-alert>
      </nb-card-body>

      <nb-card-footer>
        <div class="d-flex justify-content-end gap-2">
          <button nbButton [status]="confirmButtonStatus" (click)="confirm()">
            <nb-icon *ngIf="confirmIcon" [icon]="confirmIcon"></nb-icon>
            {{ confirmText }}
          </button>
          <button nbButton status="basic" (click)="cancel()">
            {{ cancelText }}
          </button>
        </div>
      </nb-card-footer>
    </nb-card>
  `,
  styles: [`
    .confirmation-dialog {
      min-width: 400px;
      max-width: 500px;
    }

    .message-content {
      margin-bottom: 1rem;
    }

    .form-group {
      margin-bottom: 1rem;

      label {
        display: block;
        margin-bottom: 0.5rem;
        font-weight: 500;
      }
    }

    nb-card-footer {
      padding: 1rem;
      border-top: 1px solid var(--border-basic-color);

      .gap-2 {
        gap: 0.5rem;
      }
    }

    nb-alert {
      margin-top: 1rem;
    }
  `],
})
export class ConfirmationDialogComponent {
  @Input() title: string = '确认';
  @Input() message: string = '';
  @Input() confirmText: string = '确认';
  @Input() cancelText: string = '取消';
  @Input() confirmButtonStatus: string = 'primary';
  @Input() confirmIcon: string = '';

  // Comment input configuration
  @Input() showCommentInput: boolean = false;
  @Input() commentLabel: string = '备注';
  @Input() commentPlaceholder: string = '请输入备注...';
  @Input() commentRequired: boolean = false;
  @Input() commentRows: number = 3;
  @Input() commentHint: string = '';

  // Alert configuration
  @Input() alertMessage: string = '';
  @Input() alertStatus: string = 'info';

  comment: string = '';

  constructor(protected dialogRef: NbDialogRef<ConfirmationDialogComponent>) {}

  confirm() {
    if (this.showCommentInput && this.commentRequired && !this.comment?.trim()) {
      this.alertMessage = '请填写必填项';
      this.alertStatus = 'warning';
      return;
    }

    this.dialogRef.close({
      confirmed: true,
      comment: this.comment,
    });
  }

  cancel() {
    this.dialogRef.close({
      confirmed: false,
    });
  }
}