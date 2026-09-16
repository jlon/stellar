import { Component, OnInit, inject } from '@angular/core';
import { FormBuilder, FormGroup, Validators, AbstractControl, ValidationErrors, FormsModule, ReactiveFormsModule } from '@angular/forms';
import { NbDialogRef, NbCardModule, NbIconModule, NbInputModule, NbButtonModule, NbTooltipModule } from '@nebular/theme';
import { SystemFunction } from '../../../../@core/data/system-function';


@Component({
    selector: 'ngx-edit-function-dialog',
    template: `
    <nb-card>
      <nb-card-header>
        <div class="d-flex align-items-center">
          <nb-icon icon="edit-outline" class="mr-2"></nb-icon>
          <h5 class="mb-0">编辑自定义功能</h5>
        </div>
      </nb-card-header>
      <nb-card-body>
        <form [formGroup]="editFunctionForm" (ngSubmit)="onSubmit()">
          <div class="row">
            <div class="col-md-6">
              <div class="form-group">
                <label for="category_name" class="label">分类名称 *</label>
                <input size="small"
                  type="text"
                  id="category_name"
                  formControlName="category_name"
                  nbInput
                  fullWidth
                  placeholder="请输入分类名称"
                  status="basic"
                  readonly
                  />
                <small class="text-hint">
                  编辑时分类名称不可修改
                </small>
              </div>
            </div>
            <div class="col-md-6">
              <div class="form-group">
                <label for="function_name" class="label">功能名称 *</label>
                <input size="small"
                  type="text"
                  id="function_name"
                  formControlName="function_name"
                  nbInput
                  fullWidth
                  placeholder="请输入功能名称"
                  status="basic"
                  />
                @if (editFunctionForm.get('function_name')?.invalid && editFunctionForm.get('function_name')?.touched) {
                  <div class="text-danger small mt-1">
                    功能名称是必填项
                  </div>
                }
              </div>
            </div>
          </div>
    
          <div class="form-group">
            <label for="description" class="label">功能说明 *</label>
            <textarea size="small"
              id="description"
              formControlName="description"
              nbInput
              fullWidth
              rows="3"
              placeholder="请输入功能说明"
              status="basic"
            ></textarea>
            @if (editFunctionForm.get('description')?.invalid && editFunctionForm.get('description')?.touched) {
              <div class="text-danger small mt-1">
                功能说明是必填项
              </div>
            }
          </div>
    
          <div class="form-group">
            <label for="sql_query" class="label">SQL查询 *</label>
            <textarea size="small"
              id="sql_query"
              formControlName="sql_query"
              nbInput
              fullWidth
              rows="6"
              placeholder="请输入SQL查询语句（只支持SELECT和SHOW语句）"
              status="basic"
            ></textarea>
            @if (editFunctionForm.get('sql_query')?.invalid && editFunctionForm.get('sql_query')?.touched) {
              <div class="text-danger small mt-1">
                SQL查询是必填项
              </div>
            }
            <small class="text-hint">
              只支持SELECT和SHOW类型的SQL查询语句
            </small>
          </div>
        </form>
      </nb-card-body>
      <nb-card-footer>
        <div class="d-flex justify-content-end gap-2">
          <button
            type="button"
            nbButton
            ghost
            status="basic"
            size="small"
            (click)="onCancel()"
            nbTooltip="取消" nbTooltipPlacement="top" aria-label="取消"
            class="icon-btn" type="button"
            >
            <nb-icon icon="close-outline"></nb-icon>
          </button>
          <button type="button" class="icon-btn is-primary" [disabled]="editFunctionForm.invalid"
            (click)="onSubmit()" nbTooltip="保存" nbTooltipPlacement="top" aria-label="保存"><nb-icon icon="checkmark-outline"></nb-icon></button>
        </div>
      </nb-card-footer>
    </nb-card>
    `,
    styles: [`
    :host {
      display: block;
      width: 100%;
    }
    
    .form-group {
      margin-bottom: 1.5rem;
    }
    
    .label {
      font-weight: 600;
      margin-bottom: 0.5rem;
      display: block;
      color: var(--text-basic-color);
    }
    
    .text-danger {
      color: var(--color-danger-default) !important;
    }
    
    .text-hint {
      color: var(--text-hint-color) !important;
    }
    
    .gap-2 {
      gap: 0.5rem;
    }
    
    .gap-2 > * + * {
      margin-left: 0.5rem;
    }
    
    input[readonly] {
      background-color: var(--background-basic-color-2) !important;
      color: var(--text-hint-color) !important;
      cursor: not-allowed !important;
    }
  `],
    imports: [NbCardModule, NbIconModule, FormsModule, ReactiveFormsModule, NbInputModule, NbButtonModule,
    NbTooltipModule,]
})
export class EditFunctionDialogComponent implements OnInit {
  private fb = inject(FormBuilder);
  private dialogRef = inject<NbDialogRef<EditFunctionDialogComponent>>(NbDialogRef);

  editFunctionForm: FormGroup;
  function: SystemFunction;

  constructor() {
    this.editFunctionForm = this.fb.group({
      category_name: ['', [Validators.required, Validators.maxLength(100), this.trimValidator]],
      function_name: ['', [Validators.required, Validators.maxLength(100), this.trimValidator]],
      description: ['', [Validators.required, Validators.maxLength(500), this.trimValidator]],
      sql_query: ['', [Validators.required, this.trimValidator]]
    });
  }

  // 自定义验证器：检查trim后是否为空
  private trimValidator(control: AbstractControl): ValidationErrors | null {
    if (control.value && typeof control.value === 'string') {
      const trimmed = control.value.trim();
      if (trimmed.length === 0) {
        return { required: true };
      }
    }
    return null;
  }

  ngOnInit() {
    // 预填充表单数据
    if (this.function) {
      this.editFunctionForm.patchValue({
        category_name: this.function.categoryName || '',
        function_name: this.function.functionName || '',
        description: this.function.description || '',
        sql_query: this.function.sqlQuery || ''
      });
    }
  }

  onSubmit() {
    if (this.editFunctionForm.valid) {
      const formValue = this.editFunctionForm.value;
      const updatedFunction: SystemFunction = {
        ...this.function,
        categoryName: formValue.category_name?.trim() || '',
        functionName: formValue.function_name?.trim() || '',
        description: formValue.description?.trim() || '',
        sqlQuery: formValue.sql_query?.trim() || ''
      };
      this.dialogRef.close(updatedFunction);
    }
  }

  onCancel() {
    this.dialogRef.close();
  }
}
