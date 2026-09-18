import { Injectable, inject } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { TranslateLoader, TranslationObject } from '@ngx-translate/core';
import { Observable } from 'rxjs';

/**
 * 多文件合并词表加载器：assets/i18n/{zh,en}/*.json 按命名空间拆分，
 * 加载时合并为单个词表对象（避免单文件数千行难维护）。
 */
@Injectable()
export class MergedTranslateLoader implements TranslateLoader {
  private http = inject(HttpClient);

  getTranslation(lang: string): Observable<TranslationObject> {
    // 命名空间清单；新增模块在这里追加一行
    const namespaces = ['common', 'nav', 'clusters', 'nodes', 'queries', 'sessions',
      'variables', 'mv', 'system', 'permission', 'resource-groups', 'agent', 'theme'];
    const merged: Record<string, unknown> = {};
    // 逐个加载后合并；缺文件按空词表处理（渐进翻译：未覆盖的 key 回退 fallbackLang）
    return new Observable<TranslationObject>(observer => {
      let pending = namespaces.length;
      for (const ns of namespaces) {
        this.http.get<Record<string, unknown>>(`assets/i18n/${lang}/${ns}.json`).subscribe({
          next: (dict) => {
            Object.assign(merged, dict);  // 扁平合并：key 即中文原文，无需 ns 前缀
            if (--pending === 0) observer.next(merged as TranslationObject);
          },
          error: () => {
            if (--pending === 0) observer.next(merged as TranslationObject);
          },
        });
      }
    });
  }
}
