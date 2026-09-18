#!/usr/bin/env python3
"""
i18n 模板转换器：把 HTML/TS 里的中文文案接上 ngx-translate。
- HTML: >中文< → >{{ '中文' | translate }}<；placeholder/title/aria-label="中文" → 属性绑定
- TS:   this.toastr.danger('中文') 等 → 用 I18nService.instant() 包装（注入已是模式化的场景）
策略：只转换确定安全的模式，转换不了的输出清单人工处理。
"""
import re, os, glob, json, sys

def convert_html(path, content, report):
    changed = 0
    lines = content.split('\n')
    out = []
    for i, line in enumerate(lines, 1):
        orig = line
        # 跳过已转换的
        if '| translate' in line:
            out.append(line); continue
        # 1) 属性静态中文 → 绑定。placeholder/title/aria-label（不含 [ ]）
        line = re.sub(
            r'(\s)(placeholder|title|aria-label)="([^"\{\}]*[\u4e00-\u9fff][^"\{\}]*)"',
            lambda m: f'{m.group(1)}[{m.group(2)}]="\'{m.group(3)}\' | translate"',
            line)
        # nbTooltip="中文" → nbTooltip="'中文'|translate"（指令的输入同样用属性绑定）
        line = re.sub(
            r'(\s)nbTooltip="([^"\{\}]*[\u4e00-\u9fff][^"\{\}]*)"',
            lambda m: f'{m.group(1)}[nbTooltip]="\'{m.group(2)}\' | translate"',
            line)
        # 2) 纯文本节点 >中文< 或 >中文{{x}}< → {{ '中文' | translate }}{{x}}
        #    仅当该文本节点包含中文且非混合复杂插值
        def text_repl(m):
            pre, inner, post = m.group(1), m.group(2), m.group(3)
            # 提取纯中文部分与插值
            text = inner
            if '{{' in text:
                # 混合插值：暂不转换（人工处理清单）
                report['mixed'].append((path, i, text.strip()[:60]))
                return m.group(0)
            t = text.strip()
            if not t or not re.search(r'[\u4e00-\u9fff]', t):
                return m.group(0)
            report['converted'] += 1
            return f'{pre}{{{{ \'{t}\' | translate }}}}{post}'
        line = re.sub(r'(>)((?:[^<>{}]|\{\{[^}]*\}\})*)(<)', text_repl, line)
        # 3) 纯插值文本 {{ '中文' }} 之类跳过
        if line != orig:
            changed += 1
        out.append(line)
    return '\n'.join(out), changed

def convert_ts(path, content, report):
    """TS 中常见用户可见文案场景（排除注释/URL/日志）。保守策略：跳过，输出人工清单。"""
    # TS 中文的场景太多（变量赋值、对象字面量、模板串），无法通用安全转换。
    # 策略：只处理最模式化的 nb toastr 调用: this.toastr.danger/success/warning/info('中文', '中文')
    changed = 0
    def toastr_repl(m):
        report['toastr'] += 1
        return (f"{m.group(1)}this.i18n.instant('{m.group(2)}')"
                + (f", this.i18n.instant('{m.group(3)}')" if m.group(3) else "")
                + ")")
    content = re.sub(
        r'(this\.toastr(?:Service)?\.(?:danger|success|warning|info)\()\'([^\']*[一-龥][^\']*)\'(?:,\s*\'([^\']*[一-龥][^\']*)\')?\)',
        toastr_repl, content)
    changed = content.count('this.i18n.instant(')
    return content, changed

def main():
    report = {'converted': 0, 'toastr': 0, 'mixed': []}
    total_files = 0
    for f in glob.glob('src/app/**/*.html', recursive=True):
        if '.spec.' in f: continue
        s = open(f, encoding='utf-8').read()
        new, n = convert_html(f, s, report)
        if n:
            open(f, 'w', encoding='utf-8').write(new)
            total_files += 1
    print(f'HTML 转换文件数: {total_files}, 转换点: {report["converted"]}')
    json.dump(report['mixed'][:200], open('/tmp/i18n-mixed.json', 'w'), ensure_ascii=False, indent=1)
    print(f'混合插值待人工: {len(report["mixed"])}')

if __name__ == '__main__':
    main()
