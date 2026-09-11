import { Component, Input } from '@angular/core';
import { LLMProvider } from '../../../../@core/data/llm-provider.service';
import { NbBadgeModule } from '@nebular/theme';

@Component({
    selector: 'ngx-llm-provider-status-cell',
    template: `
    <div class="d-flex flex-wrap align-items-center">
      <nb-badge
        [text]="rowData.is_active ? '已激活' : '未激活'"
        [status]="rowData.is_active ? 'success' : 'basic'"
        class="mr-1"
      ></nb-badge>
      <nb-badge
        [text]="rowData.enabled ? '启用' : '禁用'"
        [status]="rowData.enabled ? 'info' : 'warning'"
      ></nb-badge>
    </div>
  `,
    imports: [NbBadgeModule],
})
export class LLMProviderStatusCellComponent {
  @Input() rowData!: LLMProvider;
}
