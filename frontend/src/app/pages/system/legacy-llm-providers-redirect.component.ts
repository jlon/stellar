import { Component, OnInit, inject } from '@angular/core';
import { Router } from '@angular/router';

/** Keeps existing bookmarks working after AI connections moved into the assistant. */
@Component({
  selector: 'ngx-legacy-llm-providers-redirect',
  standalone: true,
  template: '',
})
export class LegacyLlmProvidersRedirectComponent implements OnInit {
  private readonly router = inject(Router);

  ngOnInit(): void {
    void this.router.navigate(['/pages/cluster-ops/agent'], {
      queryParams: { panel: 'ai-connections' },
      replaceUrl: true,
    });
  }
}
