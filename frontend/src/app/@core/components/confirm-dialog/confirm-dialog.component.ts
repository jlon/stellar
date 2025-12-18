import { Component, Input } from '@angular/core';
import { NbDialogRef } from '@nebular/theme';

@Component({
  selector: 'ngx-confirm-dialog',
  template: `
    <nb-card>
      <nb-card-header>{{ title }}</nb-card-header>
      <nb-card-body>
        <p style="white-space: pre-line;">{{ message }}</p>
      </nb-card-body>
      <nb-card-footer>
        <button nbButton status="basic" (click)="cancel()">{{ cancelText }}</button>
        <button nbButton [status]="confirmStatus" (click)="confirm()">{{ confirmText }}</button>
      </nb-card-footer>
    </nb-card>
  `,
  styles: [`
    nb-card {
      margin: 0;
      min-width: 400px;
      max-width: 600px;
    }
    
    nb-card-footer {
      display: flex;
      justify-content: flex-end;
      gap: 0.5rem;
    }
    
    p {
      margin: 0;
      line-height: 1.5;
    }
  `]
})
export class ConfirmDialogComponent {
  @Input() title: string;
  @Input() message: string;
  @Input() confirmText: string = '确定';
  @Input() cancelText: string = '取消';
  @Input() confirmStatus: string = 'primary';

  constructor(protected ref: NbDialogRef<ConfirmDialogComponent>) {}

  cancel() {
    this.ref.close(false);
  }

  confirm() {
    this.ref.close(true);
  }
}
