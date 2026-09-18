//! 主题跟随的 AI 空态插图（4 种场景，内联 SVG + CSS 变量）：
//! 深浅主题自动适配（颜色来自主题变量，无硬编码 RGB）。

import { Component, Input } from '@angular/core';
import { CommonModule } from '@angular/common';

export type AiIllustrationKind = 'chat' | 'incidents' | 'sessions' | 'float';

@Component({
  selector: 'ngx-ai-illustration',
  templateUrl: './ai-illustration.component.html',
  styleUrls: ['./ai-illustration.component.scss'],
  standalone: true,
  imports: [CommonModule],
})
export class AiIllustrationComponent {
  @Input() kind: AiIllustrationKind = 'chat';
}