import { TranslatePipe } from '@ngx-translate/core';
import { Component, Input, OnInit, inject } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { NbAlertModule, NbDialogRef, NbCardModule, NbButtonModule, NbTooltipModule, NbIconModule, NbInputModule } from '@nebular/theme';

/** 变量修改弹窗（替代原生 prompt，Nebular 原生组件）。 */
@Component({
  selector: 'ngx-variable-edit-dialog',
  templateUrl: './variable-edit-dialog.component.html',
  standalone: true,
  imports: [
    TranslatePipe,FormsModule, NbCardModule, NbButtonModule, NbIconModule, NbInputModule, NbAlertModule,
    NbTooltipModule,],
  styles: [
    `
      .variable-edit-dialog {
        width: min(30rem, calc(100vw - 2rem));
        margin: 0;
      }
      .dialog-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 1rem 1.25rem;
        font-size: 0.95rem;
        font-weight: 600;
      }
      .dialog-header button {
        width: 2rem;
        height: 2rem;
        padding: 0;
      }
      .dialog-body {
        padding: 1.25rem;
      }
      .variable-name {
        display: block;
        margin-bottom: 1rem;
        color: var(--text-basic-color);
        font-family: var(--font-family-primary);
        font-size: 0.95rem;
        font-weight: 600;
        overflow-wrap: anywhere;
      }
      .current-value {
        margin-bottom: 1rem;
        padding: 0.65rem 0.75rem;
        color: var(--text-hint-color);
        background: var(--background-basic-color-3);
        border: 1px solid var(--border-basic-color-3);
        border-radius: var(--border-radius);
        font-size: 0.8rem;
        line-height: 1.45;
        overflow-wrap: anywhere;
      }
      .guidance-alert {
        margin-bottom: 1rem;
        font-size: 0.8rem;
        line-height: 1.45;
      }
      .field-label {
        display: block;
        margin-bottom: 0.4rem;
        color: var(--text-hint-color);
        font-size: 0.75rem;
        font-weight: 600;
      }
      .dialog-footer {
        display: flex;
        justify-content: flex-end;
        gap: 0.5rem;
        padding: 0.875rem 1.25rem;
      }
    `,
  ],
})
export class VariableEditDialogComponent implements OnInit {
  private dialogRef = inject<NbDialogRef<VariableEditDialogComponent>>(NbDialogRef);

  /** 变量名（只读展示）。 */
  @Input() name = '';
  /** 当前值。 */
  @Input() value = '';
  /** 新值。 */
  newValue = '';

  ngOnInit(): void {
    this.newValue = this.value ?? '';
  }

  save(): void {
    this.dialogRef.close(this.newValue);
  }

  cancel(): void {
    this.dialogRef.close();
  }
}
