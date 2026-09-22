import { ChangeDetectorRef } from '@angular/core';
import { NavigationEnd, Router } from '@angular/router';
import { BehaviorSubject, Subject } from 'rxjs';
import { NbSidebarService, NbToastrService } from '@nebular/theme';

import { TestBed } from '@angular/core/testing';
import { AgentService } from '../../../../@core/data/agent.service';
import { AgentChatService } from '../../../../@core/data/agent-chat.service';
import { ClusterContextService } from '../../../../@core/data/cluster-context.service';
import { PermissionService } from '../../../../@core/data/permission.service';
import { I18nService } from '../../../../@core/i18n/i18n.service';
import { ChatFloatComponent } from '../chat-float.component';

describe('ChatFloatComponent', () => {
  let component: ChatFloatComponent;
  let router: { url: string; events: Subject<NavigationEnd> };
  let sidebar: {
    expand: jasmine.Spy;
    collapse: jasmine.Spy;
    onExpand: () => Subject<{ tag: string }>;
    onCollapse: () => Subject<{ tag: string }>;
  };
  let permissions: { allowed: boolean; permissions$: BehaviorSubject<unknown[]> };

  beforeEach(() => {
    router = {
      url: '/pages/starrocks/overview',
      events: new Subject<NavigationEnd>(),
    };
    const expanded = new Subject<{ tag: string }>();
    const collapsed = new Subject<{ tag: string }>();
    sidebar = {
      expand: jasmine.createSpy('expand'),
      collapse: jasmine.createSpy('collapse'),
      onExpand: () => expanded,
      onCollapse: () => collapsed,
    };
    permissions = {
      allowed: true,
      permissions$: new BehaviorSubject<unknown[]>([]),
    };
    TestBed.configureTestingModule({
      providers: [
        { provide: Router, useValue: router },
        { provide: NbSidebarService, useValue: sidebar },
        { provide: PermissionService, useValue: {
          permissions$: permissions.permissions$,
          hasPermission: () => permissions.allowed,
        } },
        { provide: AgentService, useValue: {} },
        { provide: AgentChatService, useValue: {} },
        { provide: ClusterContextService, useValue: {} },
        { provide: I18nService, useValue: { instant: (value: string) => value } },
        { provide: NbToastrService, useValue: {} },
        { provide: ChangeDetectorRef, useValue: { detectChanges: () => undefined } },
      ],
    });
    component = TestBed.runInInjectionContext(() => new ChatFloatComponent());
  });

  it('shows the launcher off the full assistant page and opens the assistant drawer', () => {
    component.ngOnInit();

    expect(component.visible).toBeTrue();
    component.openDrawer();
    expect(sidebar.expand).toHaveBeenCalledWith('assistant-drawer');
  });

  it('hides the launcher and closes the drawer on the full assistant page', () => {
    component.ngOnInit();

    router.url = '/pages/cluster-ops/agent?session=42';
    router.events.next(new NavigationEnd(1, router.url, router.url));

    expect(component.visible).toBeFalse();
    expect(sidebar.collapse).toHaveBeenCalledWith('assistant-drawer');
  });

  it('hides the launcher while the assistant drawer is expanded', () => {
    component.ngOnInit();

    sidebar.onExpand().next({ tag: 'assistant-drawer' });
    expect(component.visible).toBeFalse();

    sidebar.onCollapse().next({ tag: 'assistant-drawer' });
    expect(component.visible).toBeTrue();
  });

  it('keeps session history collapsed until the user expands it', () => {
    expect(component.historyExpanded).toBeFalse();

    component.toggleHistory();
    expect(component.historyExpanded).toBeTrue();

    component.toggleHistory();
    expect(component.historyExpanded).toBeFalse();
  });

  it('collapses an open drawer when the pointer lands outside it', () => {
    component.drawer = true;
    component.open = true;

    component.onDocumentPointerDown({ target: document.body } as unknown as PointerEvent);

    expect(sidebar.collapse).toHaveBeenCalledWith('assistant-drawer');
  });

  it('keeps Escape as the keyboard fallback to collapse an open drawer', () => {
    component.drawer = true;
    component.open = true;

    component.onEscape();

    expect(sidebar.collapse).toHaveBeenCalledWith('assistant-drawer');
  });

  it('suppresses the click that ends a launcher drag', () => {
    const launcher = document.createElement('button');
    component.onLauncherPointerDown({
      button: 0,
      currentTarget: launcher,
      pointerId: 7,
      clientX: 20,
      clientY: 20,
    } as unknown as PointerEvent);
    component.onLauncherPointerMove({
      pointerId: 7,
      clientX: 40,
      clientY: 50,
      preventDefault: () => undefined,
    } as unknown as PointerEvent);
    component.onLauncherPointerEnd({ pointerId: 7 } as unknown as PointerEvent);

    component.openDrawer();
    expect(sidebar.expand).not.toHaveBeenCalled();

    component.openDrawer();
    expect(sidebar.expand).toHaveBeenCalledWith('assistant-drawer');
  });

  it('hides the launcher after the user closes the quick entry', () => {
    component.ngOnInit();

    component.dismissLauncher(new MouseEvent('click'));

    expect(component.visible).toBeFalse();
  });
});
