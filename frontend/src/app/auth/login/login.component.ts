import { I18nService } from '../../@core/i18n/i18n.service';
import { TranslatePipe } from '@ngx-translate/core';
import { inject, AfterViewInit, ChangeDetectorRef, Component, ElementRef, OnDestroy, OnInit, ViewEncapsulation } from '@angular/core';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { FormsModule } from '@angular/forms';

import { NbAlertModule, NbButtonModule, NbCheckboxModule, NbIconModule, NbInputModule, NbToastrService } from '@nebular/theme';
import { AuthService } from '../../@core/data/auth.service';

@Component({
  standalone: true,
  selector: 'ngx-login',
  templateUrl: './login.component.html',
  styleUrls: ['./login.component.scss'],
  imports: [
    TranslatePipe,
    NbAlertModule,
    FormsModule,
    NbInputModule,
    NbCheckboxModule,
    NbButtonModule,
    NbIconModule,
    RouterLink
],
  encapsulation: ViewEncapsulation.None,
})
export class LoginComponent implements OnInit, AfterViewInit, OnDestroy {
  private i18n = inject(I18nService);
  submitted = false;
  user = {
    username: '',
    password: '',
  };
  rememberMe = false;
  errors: string[] = [];
  messages: string[] = [];
  showMessages = false;
  returnUrl: string;
  panelWidth = 544;
  isResizing = false;
  private resizeStartX = 0;
  private resizeStartWidth = 0;
  private removeListeners: Array<() => void> = [];
  private readonly minPanelWidth = 360;
  private readonly minVisualWidth = 280;

  protected readonly router = inject(Router);
  private readonly route = inject(ActivatedRoute);
  private readonly authService = inject(AuthService);
  private readonly toastrService = inject(NbToastrService);
  private readonly el = inject(ElementRef<HTMLElement>);
  private readonly cdr = inject(ChangeDetectorRef);

  ngOnInit(): void {
    const rawReturnUrl = this.route.snapshot.queryParams['returnUrl'];
    this.returnUrl = this.authService.normalizeReturnUrl(rawReturnUrl);

    const savedUsername = localStorage.getItem('remembered_username');
    if (savedUsername) {
      this.user.username = savedUsername;
      this.rememberMe = true;
    }

    if (this.authService.isAuthenticated()) {
      this.router.navigateByUrl(this.returnUrl, { replaceUrl: true });
    }
  }

  ngAfterViewInit(): void {
    this.setupResize();
  }

  ngOnDestroy(): void {
    this.removeListeners.forEach(fn => fn());
  }

  private setupResize(): void {
    const panel = this.el.nativeElement.querySelector('.login-panel');
    const handle = this.el.nativeElement.querySelector('.panel-resize-handle');
    if (!(panel instanceof HTMLElement) || !(handle instanceof HTMLElement)) {
      return;
    }

    const onPointerDown = (event: PointerEvent) => {
      event.preventDefault();
      this.isResizing = true;
      this.resizeStartX = event.clientX;
      this.resizeStartWidth = panel.getBoundingClientRect().width;
      this.cdr.detectChanges();
    };

    const onPointerMove = (event: PointerEvent) => {
      if (!this.isResizing) {
        return;
      }

      const nextWidth = this.resizeStartWidth + (this.resizeStartX - event.clientX);
      const maxWidth = window.innerWidth - this.minVisualWidth;
      this.panelWidth = Math.min(maxWidth, Math.max(this.minPanelWidth, nextWidth));
      this.cdr.detectChanges();
    };

    const onPointerUp = () => {
      if (!this.isResizing) {
        return;
      }

      this.isResizing = false;
      this.cdr.detectChanges();
    };

    handle.addEventListener('pointerdown', onPointerDown);
    document.addEventListener('pointermove', onPointerMove);
    document.addEventListener('pointerup', onPointerUp);
    document.addEventListener('pointercancel', onPointerUp);

    this.removeListeners.push(
      () => handle.removeEventListener('pointerdown', onPointerDown),
      () => document.removeEventListener('pointermove', onPointerMove),
      () => document.removeEventListener('pointerup', onPointerUp),
      () => document.removeEventListener('pointercancel', onPointerUp),
    );
  }

  login(): void {
    this.errors = [];
    this.messages = [];
    this.submitted = true;

    if (!this.user.username || !this.user.password) {
      this.errors.push('请输入用户名和密码');
      this.submitted = false;
      return;
    }

    this.authService.login(this.user).subscribe({
      next: () => {
        this.submitted = false;

        if (this.rememberMe) {
          localStorage.setItem('remembered_username', this.user.username);
        } else {
          localStorage.removeItem('remembered_username');
        }

        this.toastrService.success(this.i18n.instant('欢迎回来，正在进入控制台。'), this.i18n.instant('登录成功'));
        setTimeout(() => {
          this.router.navigateByUrl(this.returnUrl, { replaceUrl: true });
        }, 500);
      },
      error: (error) => {
        this.submitted = false;
        const errorMessage = error.error?.message || '登录未成功，请检查用户名和密码后重试。';
        this.errors = [errorMessage];
        this.showMessages = true;
      },
    });
  }
}
