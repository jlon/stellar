import { Component, OnInit, OnDestroy, ViewChild } from '@angular/core';
import { FormBuilder, FormGroup, FormArray, Validators } from '@angular/forms';
import { ActivatedRoute, Router } from '@angular/router';
import { NbToastrService, NbTabsetComponent } from '@nebular/theme';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';

import { ResourceGroupService } from '../resource-group.service';
import {
  ResourceGroup,
  CreateResourceGroupRequest,
  UpdateResourceGroupRequest,
  ClassifierRequest,
} from '../models/resource-group.model';

@Component({
  selector: 'ngx-resource-group-form',
  templateUrl: './resource-group-form.component.html',
  styleUrls: ['./resource-group-form.component.scss'],
})
export class ResourceGroupFormComponent implements OnInit, OnDestroy {
  @ViewChild('tabset') tabset: NbTabsetComponent;
  
  private destroy$ = new Subject<void>();

  form: FormGroup;
  loading = false;
  isEditMode = false;
  resourceGroupName: string | null = null;

  queryTypeOptions = [
    { value: 'SELECT', label: 'SELECT' },
    { value: 'INSERT', label: 'INSERT' },
    { value: 'UPDATE', label: 'UPDATE' },
    { value: 'DELETE', label: 'DELETE' },
  ];

  constructor(
    private fb: FormBuilder,
    private route: ActivatedRoute,
    private router: Router,
    private resourceGroupService: ResourceGroupService,
    private toastrService: NbToastrService,
  ) {
    this.initForm();
  }

  ngOnInit(): void {
    this.resourceGroupName = this.route.snapshot.paramMap.get('name');
    this.isEditMode = !!this.resourceGroupName;

    if (this.isEditMode && this.resourceGroupName) {
      this.loadResourceGroup(this.resourceGroupName);
    }
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  private initForm(): void {
    this.form = this.fb.group({
      name: ['', [Validators.required, Validators.pattern(/^[a-zA-Z0-9_]+$/)]],
      cpu_weight: [null, [Validators.min(1), Validators.max(100)]],
      exclusive_cpu_cores: [null, [Validators.min(0)]],
      mem_limit: [''],
      big_query_cpu_second_limit: [null, [Validators.min(0)]],
      big_query_scan_rows_limit: [null, [Validators.min(0)]],
      big_query_mem_limit: [''],
      concurrency_limit: [null, [Validators.min(1)]],
      spill_mem_limit_threshold: [''],
      classifiers: this.fb.array([]),
    });

    if (this.isEditMode) {
      this.form.get('name')?.disable();
    }
  }

  get classifiers(): FormArray {
    return this.form.get('classifiers') as FormArray;
  }

  private loadResourceGroup(name: string): void {
    this.loading = true;
    this.resourceGroupService
      .getResourceGroup(name)
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: (group) => {
          this.populateForm(group);
          this.loading = false;
        },
        error: (error) => {
          console.error('Failed to load resource group:', error);
          this.toastrService.danger('加载资源组失败', '错误');
          this.loading = false;
        },
      });
  }

  private populateForm(group: ResourceGroup): void {
    this.form.patchValue({
      name: group.name,
      cpu_weight: group.cpu_weight,
      exclusive_cpu_cores: group.exclusive_cpu_cores,
      mem_limit: group.mem_limit,
      big_query_cpu_second_limit: group.big_query_cpu_second_limit,
      big_query_scan_rows_limit: group.big_query_scan_rows_limit,
      big_query_mem_limit: group.big_query_mem_limit,
      concurrency_limit: group.concurrency_limit,
      spill_mem_limit_threshold: group.spill_mem_limit_threshold,
    });

    // 清空现有分类器
    while (this.classifiers.length !== 0) {
      this.classifiers.removeAt(0);
    }

    // 添加现有分类器
    group.classifiers.forEach((classifier) => {
      this.classifiers.push(
        this.fb.group({
          user: [classifier.user || ''],
          role: [classifier.role || ''],
          query_type: [classifier.query_type ? classifier.query_type.split(',') : []],
          source_ip: [classifier.source_ip || ''],
          db: [classifier.db || ''],
          weight: [classifier.weight || 1, [Validators.min(1)]],
        }),
      );
    });
  }

  addClassifier(): void {
    this.classifiers.push(
      this.fb.group({
        user: [''],
        role: [''],
        query_type: [[]],
        source_ip: [''],
        db: [''],
        weight: [1, [Validators.min(1)]],
      }),
    );
  }

  removeClassifier(index: number): void {
    this.classifiers.removeAt(index);
  }

  onSubmit(): void {
    if (this.form.invalid) {
      this.markFormGroupTouched(this.form);
      this.navigateToFirstInvalidTab();
      this.toastrService.warning('请填写必填字段', '提示');
      return;
    }

    const formValue = this.form.value;
    
    const hasResourceLimit = 
      formValue.cpu_weight || 
      formValue.exclusive_cpu_cores || 
      formValue.mem_limit || 
      formValue.concurrency_limit;
    
    if (!hasResourceLimit) {
      this.toastrService.warning('请至少配置一个资源限制（CPU 权重、独占 CPU 核数、内存限制或并发限制）', '配置不完整');
      this.tabset.selectTab(this.tabset.tabs.toArray()[1]);
      return;
    }
    
    const hasClassifier = formValue.classifiers && formValue.classifiers.length > 0;
    
    if (!hasClassifier) {
      this.toastrService.warning('请至少添加一个分类器，否则查询无法分配到此资源组', '配置不完整');
      this.tabset.selectTab(this.tabset.tabs.last);
      return;
    }

    this.loading = true;

    if (this.isEditMode && this.resourceGroupName) {
      this.updateResourceGroup(formValue);
    } else {
      this.createResourceGroup(formValue);
    }
  }

  private navigateToFirstInvalidTab(): void {
    const nameControl = this.form.get('name');
    
    if (nameControl?.invalid) {
      this.tabset.selectTab(this.tabset.tabs.first);
      return;
    }
  }

  private createResourceGroup(formValue: any): void {
    const request: CreateResourceGroupRequest = {
      name: formValue.name,
      cpu_weight: formValue.cpu_weight || undefined,
      exclusive_cpu_cores: formValue.exclusive_cpu_cores || undefined,
      mem_limit: formValue.mem_limit || undefined,
      big_query_cpu_second_limit: formValue.big_query_cpu_second_limit || undefined,
      big_query_scan_rows_limit: formValue.big_query_scan_rows_limit || undefined,
      big_query_mem_limit: formValue.big_query_mem_limit || undefined,
      concurrency_limit: formValue.concurrency_limit || undefined,
      spill_mem_limit_threshold: formValue.spill_mem_limit_threshold || undefined,
      classifiers: this.buildClassifiers(formValue.classifiers),
    };

    this.resourceGroupService
      .createResourceGroup(request)
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: () => {
          this.toastrService.success('资源组创建成功', '成功');
          this.router.navigate(['/pages/cluster-ops/resource-groups']);
        },
        error: (error) => {
          console.error('Failed to create resource group:', error);
          this.toastrService.danger('创建资源组失败', '错误');
          this.loading = false;
        },
      });
  }

  private updateResourceGroup(formValue: any): void {
    const request: UpdateResourceGroupRequest = {
      cpu_weight: formValue.cpu_weight || undefined,
      exclusive_cpu_cores: formValue.exclusive_cpu_cores || undefined,
      mem_limit: formValue.mem_limit || undefined,
      big_query_cpu_second_limit: formValue.big_query_cpu_second_limit || undefined,
      big_query_scan_rows_limit: formValue.big_query_scan_rows_limit || undefined,
      big_query_mem_limit: formValue.big_query_mem_limit || undefined,
      concurrency_limit: formValue.concurrency_limit || undefined,
      spill_mem_limit_threshold: formValue.spill_mem_limit_threshold || undefined,
      add_classifiers: this.buildClassifiers(formValue.classifiers),
    };

    this.resourceGroupService
      .updateResourceGroup(this.resourceGroupName!, request)
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: () => {
          this.toastrService.success('资源组更新成功', '成功');
          this.router.navigate(['/pages/cluster-ops/resource-groups']);
        },
        error: (error) => {
          console.error('Failed to update resource group:', error);
          this.toastrService.danger('更新资源组失败', '错误');
          this.loading = false;
        },
      });
  }

  private buildClassifiers(classifiersValue: any[]): ClassifierRequest[] {
    return classifiersValue
      .filter((c) => c.user || c.role || c.query_type?.length || c.source_ip || c.db)
      .map((c) => ({
        user: c.user || undefined,
        role: c.role || undefined,
        query_type: c.query_type?.length ? c.query_type : undefined,
        source_ip: c.source_ip || undefined,
        db: c.db || undefined,
        weight: c.weight || 1,
      }));
  }

  private markFormGroupTouched(formGroup: FormGroup): void {
    Object.keys(formGroup.controls).forEach((key) => {
      const control = formGroup.get(key);
      control?.markAsTouched();

      if (control instanceof FormGroup) {
        this.markFormGroupTouched(control);
      } else if (control instanceof FormArray) {
        control.controls.forEach((arrayControl) => {
          if (arrayControl instanceof FormGroup) {
            this.markFormGroupTouched(arrayControl);
          }
        });
      }
    });
  }

  cancel(): void {
    this.router.navigate(['/pages/cluster-ops/resource-groups']);
  }
}