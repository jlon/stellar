import { Component, OnDestroy, OnInit, inject } from '@angular/core';
import { Router } from '@angular/router';
import { NbMediaBreakpointsService, NbMenuService, NbSidebarService, NbThemeService, NbToastrService, NbIconModule, NbSelectModule, NbOptionModule, NbActionsModule, NbUserModule, NbContextMenuModule } from '@nebular/theme';

import { LayoutService } from '../../../@core/utils';
import { AuthService } from '../../../@core/data/auth.service';
import { persistTheme } from '../../styles/theme-preference';
import { map, takeUntil, filter } from 'rxjs/operators';
import { Subject } from 'rxjs';

import { ClusterSelectorComponent } from '../cluster-selector/cluster-selector.component';
import { NbSecurityModule } from '@nebular/security';

@Component({
    selector: 'ngx-header',
    styleUrls: ['./header.component.scss'],
    templateUrl: './header.component.html',
    imports: [
    NbIconModule,
    NbSelectModule,
    NbOptionModule,
    ClusterSelectorComponent,
    NbActionsModule,
    NbSecurityModule,
    NbUserModule,
    NbContextMenuModule
],
})
export class HeaderComponent implements OnInit, OnDestroy {
  private sidebarService = inject(NbSidebarService);
  private menuService = inject(NbMenuService);
  private themeService = inject(NbThemeService);
  private authService = inject(AuthService);
  private layoutService = inject(LayoutService);
  private breakpointService = inject(NbMediaBreakpointsService);
  private router = inject(Router);
  private toastr = inject(NbToastrService);


  private destroy$: Subject<void> = new Subject<void>();
  userPictureOnly: boolean = false;
  user: any;

  themes = [
    {
      value: 'default',
      name: '浅色',
    },
    {
      value: 'dark',
      name: '深色',
    },
    {
      value: 'cosmic',
      name: '星空',
    },
    {
      value: 'corporate',
      name: '企业',
    },
  ];

  currentTheme = 'cosmic';

  userMenu = [
    { title: '用户设置', icon: 'settings-outline', data: { id: 'settings' } },
    { title: '退出登录', icon: 'log-out-outline', data: { id: 'logout' } },
  ];

  ngOnInit() {
    this.currentTheme = this.themeService.currentTheme;

    // Get current user info
    this.authService.currentUser
      .pipe(takeUntil(this.destroy$))
      .subscribe(currentUser => {
        if (currentUser) {
          this.user = {
            name: currentUser.username || 'admin',
            picture: currentUser.avatar || 'assets/images/nick.png', // 使用用户的头像，如果没有则使用默认头像
          };
        } else {
          // 如果未登录，设置默认用户
          this.user = {
            name: 'admin',
            picture: 'assets/images/nick.png',
          };
        }
      });

    // Handle user menu clicks
    this.menuService.onItemClick()
      .pipe(
        filter(({ tag }) => tag === 'user-context-menu'),
        map(({ item }) => item),
        takeUntil(this.destroy$),
      )
      .subscribe(item => {
        if (item.data) {
          switch (item.data.id) {
            case 'settings':
              this.router.navigate(['/pages/user-settings']);
              break;
            case 'logout':
              this.logout();
              break;
          }
        }
      });

    const { xl } = this.breakpointService.getBreakpointsMap();
    this.themeService.onMediaQueryChange()
      .pipe(
        map(([, currentBreakpoint]) => currentBreakpoint.width < xl),
        takeUntil(this.destroy$),
      )
      .subscribe((isLessThanXl: boolean) => this.userPictureOnly = isLessThanXl);

    this.themeService.onThemeChange()
      .pipe(
        map(({ name }) => name),
        takeUntil(this.destroy$),
      )
      .subscribe(themeName => this.currentTheme = themeName);
  }

  ngOnDestroy() {
    this.destroy$.next();
    this.destroy$.complete();
  }

  changeTheme(themeName: string) {
    persistTheme(themeName);
    this.themeService.changeTheme(themeName);
  }

  toggleSidebar(): boolean {
    this.sidebarService.toggle(true, 'menu-sidebar');
    this.layoutService.changeLayoutSize();

    return false;
  }

  navigateHome() {
    this.menuService.navigateHome();
    return false;
  }

  logout() {
    this.toastr.success('退出登录成功', '提示');
    setTimeout(() => {
      this.authService.logout();
    }, 500);
  }
}
