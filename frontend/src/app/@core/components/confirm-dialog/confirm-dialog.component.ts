import { Component, Input, inject } from '@angular/core';
import { NbDialogRef, NbCardModule, NbButtonModule, NbTooltipModule, NbIconModule } from '@nebular/theme';

@Component({
    selector: 'ngx-confirm-dialog',
    template: `
    <nb-card>
      <nb-card-header>{{ title }}</nb-card-header>
      <nb-card-body>
        <p style="white-space: pre-line;">{{ message }}</p>
      </nb-card-body>
      <nb-card-footer>
        <button type="button" class="icon-btn" (click)="cancel()" [nbTooltip]="cancelText" nbTooltipPlacement="top" [attr.aria-label]="cancelText"><nb-icon icon="close-outline"></nb-icon></button>
        <button type="button" class="icon-btn is-primary" (click)="confirm()" [nbTooltip]="confirmText" nbTooltipPlacement="top" [attr.aria-label]="confirmText"><nb-icon icon="checkmark-outline"></nb-icon></button>
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
    imports: [NbCardModule, NbButtonModule,
    NbTooltipModule,
    NbIconModule,]
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
