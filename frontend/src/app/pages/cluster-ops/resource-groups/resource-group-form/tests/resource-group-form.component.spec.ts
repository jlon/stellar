import { TestBed } from '@angular/core/testing';
import { NbDialogRef, NbToastrService } from '@nebular/theme';
import { of } from 'rxjs';

import { I18nService } from '../../../../../@core/i18n/i18n.service';
import { ResourceGroup } from '../../models/resource-group.model';
import { ResourceGroupService } from '../../resource-group.service';
import { ResourceGroupFormComponent } from '../resource-group-form.component';

describe('ResourceGroupFormComponent', () => {
  let component: ResourceGroupFormComponent;
  let resourceGroups: { createResourceGroup: jasmine.Spy; updateResourceGroup: jasmine.Spy };

  beforeEach(() => {
    resourceGroups = {
      createResourceGroup: jasmine.createSpy('createResourceGroup').and.returnValue(of(void 0)),
      updateResourceGroup: jasmine.createSpy('updateResourceGroup').and.returnValue(of(void 0)),
    };
    TestBed.configureTestingModule({
      providers: [
        { provide: NbDialogRef, useValue: { close: jasmine.createSpy('close') } },
        {
          provide: NbToastrService,
          useValue: { success: jasmine.createSpy('success'), warning: jasmine.createSpy('warning') },
        },
        { provide: I18nService, useValue: { instant: (value: string) => value } },
        { provide: ResourceGroupService, useValue: resourceGroups },
      ],
    });
    component = TestBed.runInInjectionContext(() => new ResourceGroupFormComponent());
  });

  it('requires a memory limit before creation', () => {
    const memLimit = component.form.get('mem_limit');

    expect(memLimit?.hasError('required')).toBeTrue();
    memLimit?.setValue('50%');
    expect(memLimit?.valid).toBeTrue();
  });

  it('requires a classifier before creation', () => {
    component.form.patchValue({ name: 'analytics', cpu_weight: 1, mem_limit: '50%' });
    (component as any).tabset = { selectTab: jasmine.createSpy('selectTab'), tabs: { last: {} } };

    component.onSubmit();

    expect(resourceGroups.createResourceGroup).not.toHaveBeenCalled();
  });

  it('treats an inactive CPU mode as unset when editing', () => {
    const group: ResourceGroup = {
      name: 'exclusive_group',
      id: 1,
      cpu_weight: 0,
      exclusive_cpu_cores: 2,
      mem_limit: '50%',
      classifiers: [],
    };

    (component as any).populateForm(group);

    expect(component.form.get('cpu_weight')?.value).toBeNull();
    expect(component.form.valid).toBeTrue();
  });

  it('clears the inactive CPU mode when switching CPU allocation modes', () => {
    component.resourceGroupName = 'shared_group';
    component.form.patchValue({
      cpu_weight: null,
      exclusive_cpu_cores: 2,
      mem_limit: '50%',
    });

    (component as any).updateResourceGroup(component.form.value);

    expect(resourceGroups.updateResourceGroup).toHaveBeenCalledWith(
      'shared_group',
      jasmine.objectContaining({ cpu_weight: 0, exclusive_cpu_cores: 2 }),
    );
  });

  it('allows an edited resource group to remove all classifiers', () => {
    const group: ResourceGroup = {
      name: 'session_selected_group',
      id: 1,
      cpu_weight: 1,
      mem_limit: '50%',
      classifiers: [{ id: 7, weight: 1, user: 'analytics' }],
    };
    component.resourceGroupName = group.name;
    (component as any).populateForm(group);
    component.removeClassifier(0);

    component.onSubmit();

    expect(resourceGroups.updateResourceGroup).toHaveBeenCalledWith(
      'session_selected_group',
      jasmine.objectContaining({ drop_classifier_ids: [7] }),
    );
  });

  it('replaces a changed classifier instead of re-adding every classifier', () => {
    const group: ResourceGroup = {
      name: 'analytics',
      id: 1,
      cpu_weight: 2,
      exclusive_cpu_cores: 0,
      mem_limit: '50%',
      classifiers: [{ id: 7, weight: 1, user: 'old_user' }],
    };
    component.resourceGroupName = group.name;
    (component as any).populateForm(group);
    const user = component.classifiers.at(0).get('user');
    user?.setValue('new_user');
    user?.markAsDirty();

    (component as any).updateResourceGroup(component.form.value);

    expect(resourceGroups.updateResourceGroup).toHaveBeenCalledWith(
      'analytics',
      jasmine.objectContaining({
        add_classifiers: [jasmine.objectContaining({ user: 'new_user' })],
        drop_classifier_ids: [7],
      }),
    );
  });
});
