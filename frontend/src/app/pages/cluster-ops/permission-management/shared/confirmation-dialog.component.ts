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
        <h6>{{ title }}</h6>
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
            <textarea nbInput
              fieldSize="small"
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
        <button type="button" nbButton ghost size="small" status="basic" (click)="cancel()">
          {{ cancelText }}
        </button>
        <button type="button" nbButton size="small" [status]="confirmButtonStatus" (click)="confirm()">
          <nb-icon [icon]="confirmIcon || 'checkmark-outline'"></nb-icon>
          {{ confirmText }}
        </button>
      </nb-card-footer>
    </nb-card>
    `,
    styles: [`
    .confirmation-dialog {
      min-width: min(28rem, calc(100vw - 2rem));
      max-width: calc(100vw - 2rem);
      margin: 0;
    }

    nb-card-header {
      padding: 0.875rem 1rem;

      h6 {
        margin: 0;
        font-size: 0.9375rem;
      }
    }

    nb-card-body {
      padding: 1rem;
    }

    .message-content {
      margin-bottom: 1rem;

      p {
        margin: 0;
        color: var(--text-basic-color);
        font-size: 0.875rem;
        line-height: 1.5;
      }
    }

    .form-group {
      margin: 0;

      label {
        display: block;
        margin-bottom: 0.375rem;
        color: var(--text-hint-color);
        font-size: 0.75rem;
        font-weight: 600;
      }
    }

    nb-card-footer {
      display: flex;
      justify-content: flex-end;
      gap: 0.5rem;
      padding: 0.75rem 1rem;
      border-top: 1px solid var(--border-basic-color-3);
    }

    nb-alert {
      margin-top: 0.75rem;
    }
  `],
    imports: [
    NbCardModule,
    NbInputModule,
    FormsModule,
    NbAlertModule,
    NbButtonModule,
    NbIconModule,],
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
