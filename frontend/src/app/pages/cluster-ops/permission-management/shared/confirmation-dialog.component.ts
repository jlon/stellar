import { Component, Input, inject } from '@angular/core';
import { NbDialogRef, NbCardModule, NbInputModule, NbAlertModule, NbButtonModule, NbIconModule } from '@nebular/theme';

import { FormsModule } from '@angular/forms';

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
        @if (message) {
          <div class="message-content">
            <p>{{ message }}</p>
          </div>
        }
    
        @if (showCommentInput) {
          <div class="form-group">
            <label for="comment">
              {{ commentLabel }}
              @if (commentRequired) {
                <span class="text-danger">*</span>
              }
            </label>
            <textarea nbInput size="small"
              nbInput
              fullWidth
              [rows]="commentRows"
              [(ngModel)]="comment"
              [placeholder]="commentPlaceholder"
              [required]="commentRequired"
              id="comment">
            </textarea>
            @if (commentHint) {
              <small class="form-text text-muted">
                {{ commentHint }}
              </small>
            }
          </div>
        }
    
        @if (alertMessage) {
          <nb-alert [status]="alertStatus" appearance="outline">
            {{ alertMessage }}
          </nb-alert>
        }
      </nb-card-body>
    
      <nb-card-footer>
        <div class="d-flex justify-content-end gap-2">
          <button nbButton [status]="confirmButtonStatus" (click)="confirm()">
            @if (confirmIcon) {
              <nb-icon [icon]="confirmIcon"></nb-icon>
            }
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
    imports: [
    NbCardModule,
    NbInputModule,
    FormsModule,
    NbAlertModule,
    NbButtonModule,
    NbIconModule
],
})
export class ConfirmationDialogComponent {
  protected dialogRef = inject<NbDialogRef<ConfirmationDialogComponent>>(NbDialogRef);

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
