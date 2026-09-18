import { Injectable, inject } from '@angular/core';
import { TranslateService } from '@ngx-translate/core';
import { BehaviorSubject } from 'rxjs';

export type Lang = 'zh' | 'en';

const LANG_KEY = 'stellar.lang';
const SUPPORTED: Lang[] = ['zh', 'en'];

/** 全站语言服务：切换 + 持久化 + 通知后端 Accept-Language。 */
@Injectable({ providedIn: 'root' })
export class I18nService {
  private translate = inject(TranslateService);

  private langSubject = new BehaviorSubject<Lang>(this.stored());
  /** 当前语言（响应式，语言切换器订阅） */
  lang$ = this.langSubject.asObservable();

  /** 初始化：设当前语言。App 启动时调用一次。 */
  init(): void {
    const lang = this.stored();
    this.translate.use(lang).subscribe();
    // 同步浏览器 Intl 格式化 locale
    document.documentElement.lang = lang === 'zh' ? 'zh-CN' : 'en';
  }

  get lang(): Lang {
    return this.langSubject.value;
  }

  /** 便捷同步翻译（TS 代码里用；HTML 模板用 translate pipe） */
  instant(key: string, params?: Record<string, unknown>): string {
    return this.translate.instant(key, params);
  }

  /** 切换语言：持久化 + 重新加载词表 + 通知后端 */
  setLang(lang: Lang): void {
    if (!SUPPORTED.includes(lang) || lang === this.langSubject.value) {
      return;
    }
    localStorage.setItem(LANG_KEY, lang);
    this.translate.use(lang).subscribe(() => this.langSubject.next(lang));
    document.documentElement.lang = lang === 'zh' ? 'zh-CN' : 'en';
    this.notifyBackend(lang);
  }

  /** 后端按 Accept-Language 返回对应语言的错误/提示消息（fetch 直调，避免与拦截器循环依赖） */
  private notifyBackend(lang: Lang): void {
    fetch('/api/user/locale', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ locale: lang === 'zh' ? 'zh-CN' : 'en-US' }),
    }).catch(() => undefined);
  }

  private stored(): Lang {
    const v = localStorage.getItem(LANG_KEY) as Lang | null;
    return v && SUPPORTED.includes(v) ? v : 'zh';
  }
}
