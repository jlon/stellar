import { Component } from '@angular/core';
import { LLMProvider } from '../../../../@core/data/llm-provider.service';

@Component({
    selector: 'ngx-llm-provider-status-cell',
    template: `
    <div class="status-cell">
      <span [class]="'metric-badge ' + (rowData?.is_active ? 'metric-badge--good' : 'metric-badge--neutral')">
        {{ rowData?.is_active ? '已激活' : '未激活' }}
      </span>
      <span [class]="'metric-badge ' + (rowData?.enabled ? 'metric-badge--info' : 'metric-badge--warn')">
        {{ rowData?.enabled ? '启用' : '禁用' }}
      </span>
    </div>
  `,
    styles: [`
      .status-cell {
        display: flex;
        align-items: center;
        gap: 0.25rem;
        white-space: nowrap;
      }
    `],
})
export class LLMProviderStatusCellComponent {
  rowData!: LLMProvider;
}
