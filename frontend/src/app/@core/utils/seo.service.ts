import { Injectable, PLATFORM_ID, OnDestroy, inject } from '@angular/core';
import { isPlatformBrowser } from '@angular/common';
import { NavigationEnd, Router } from '@angular/router';
import { NB_DOCUMENT } from '@nebular/theme';
import { filter, takeUntil } from 'rxjs/operators';
import { Subject } from 'rxjs';

@Injectable()
export class SeoService implements OnDestroy {
  private router = inject(Router);


  private readonly destroy$ = new Subject<void>();
  private readonly dom: Document;
  private readonly isBrowser: boolean;
  private linkCanonical: HTMLLinkElement;

  constructor() {
    const document = inject(NB_DOCUMENT);
    const platformId = inject(PLATFORM_ID);

    this.isBrowser = isPlatformBrowser(platformId);
    this.dom = document;

    if (this.isBrowser) {
      this.createCanonicalTag();
    }
  }

  ngOnDestroy() {
    this.destroy$.next();
    this.destroy$.complete();
  }

  createCanonicalTag() {
    this.linkCanonical = this.dom.createElement('link');
    this.linkCanonical.setAttribute('rel', 'canonical');
    this.dom.head.appendChild(this.linkCanonical);
    this.linkCanonical.setAttribute('href', this.getCanonicalUrl());
  }

  trackCanonicalChanges() {
    if (!this.isBrowser) {
      return;
    }

    this.router.events.pipe(
      filter((event) => event instanceof NavigationEnd),
      takeUntil(this.destroy$),
    )
      .subscribe(() => {
        this.linkCanonical.setAttribute('href', this.getCanonicalUrl());
      });
  }

  private getCanonicalUrl(): string {
    return this.dom.location.origin + this.dom.location.pathname;
  }
}
