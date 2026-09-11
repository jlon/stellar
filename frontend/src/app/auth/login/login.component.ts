import { Component, OnInit, inject } from '@angular/core';
import { Router, ActivatedRoute, RouterLink } from '@angular/router';
import { NbToastrService, NbAlertModule, NbInputModule, NbCheckboxModule, NbButtonModule } from '@nebular/theme';
import { AuthService } from '../../@core/data/auth.service';

import { FormsModule } from '@angular/forms';

@Component({
    selector: 'ngx-login',
    templateUrl: './login.component.html',
    styleUrls: ['./login.component.scss'],
    imports: [NbAlertModule, FormsModule, NbInputModule, NbCheckboxModule, NbButtonModule, RouterLink]
})
export class LoginComponent implements OnInit {
  protected router = inject(Router);
  private route = inject(ActivatedRoute);
  private authService = inject(AuthService);
  private toastrService = inject(NbToastrService);

  submitted = false;
  user = {
    username: '',
    password: ''
  };
  rememberMe = false;
  errors: string[] = [];
  messages: string[] = [];
  showMessages = false;
  returnUrl: string;

  ngOnInit() {
    const rawReturnUrl = this.route.snapshot.queryParams['returnUrl'];
    this.returnUrl = this.authService.normalizeReturnUrl(rawReturnUrl);
    
    // Load saved username if remember me was checked
    const savedUsername = localStorage.getItem('remembered_username');
    if (savedUsername) {
      this.user.username = savedUsername;
      this.rememberMe = true;
    }
    
    // If already logged in, redirect to return URL using absolute navigation
    if (this.authService.isAuthenticated()) {
      this.router.navigateByUrl(this.returnUrl, { replaceUrl: true });
    }
  }

  login(): void {
    this.errors = [];
    this.messages = [];
    this.submitted = true;

    if (!this.user.username || !this.user.password) {
      this.errors.push('Username and password are required!');
      this.submitted = false;
      return;
    }

    this.authService.login(this.user).subscribe({
      next: (response) => {
        this.submitted = false;
        
        // Handle remember me functionality
        if (this.rememberMe) {
          localStorage.setItem('remembered_username', this.user.username);
        } else {
          localStorage.removeItem('remembered_username');
        }
        
        // Show single toast notification for login success
        this.toastrService.success('Welcome back!', 'Login Successful');
        // Navigate to return URL using absolute navigation to prevent path duplication
        setTimeout(() => {
          this.router.navigateByUrl(this.returnUrl, { replaceUrl: true });
        }, 500);
      },
      error: (error) => {
        this.submitted = false;
        // Show error in alert (form validation errors use alert, API errors use alert too for consistency)
        const errorMessage = error.error?.message || 'Login failed. Please check your credentials.';
        this.errors = [errorMessage];
        this.showMessages = true;
        // Don't show toast for API errors since we already show alert
        // this.toastrService.danger(errorMessage, 'Login Failed');
      }
    });
  }
}
