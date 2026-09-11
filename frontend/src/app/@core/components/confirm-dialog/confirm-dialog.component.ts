import { Component, Input, inject } from '@angular/core';
import { NbDialogRef, NbCardModule, NbButtonModule } from '@nebular/theme';

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
  `],
    imports: [NbCardModule, NbButtonModule]
})
export class ConfirmDialogComponent {
  protected ref = inject<NbDialogRef<ConfirmDialogComponent>>(NbDialogRef);

  @Input() title: string;
  @Input() message: string;
  @Input() confirmText: string = '确定';
  @Input() cancelText: string = '取消';
  @Input() confirmStatus: string = 'primary';

  cancel() {
    this.ref.close(false);
  }

  confirm() {
    this.ref.close(true);
  }
}
