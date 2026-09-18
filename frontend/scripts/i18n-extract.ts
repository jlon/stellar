import { Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { firstValueFrom } from 'rxjs';

/**
 * i18n 词表生成器（构建期工具，非运行时依赖）：
 * 扫描 src/app 全部 HTML/TS 中的中文文本，生成 en 词表骨架（中文→中文占位），
 * 供 LLM/人工批量翻译后写回。也用于校验遗漏。
 *
 * 运行: npx ts-node scripts/i18n-extract.ts 或 node 脚本
 */

interface ExtractedString {
  file: string;
  line: number;
  text: string;
  kind: 'html-text' | 'attr' | 'ts-string' | 'ts-template';
  ns: string;
}

// 命名空间 → 文件路径映射
function nsFor(file: string): string {
  if (file.includes('pages-menu')) return 'nav';
  if (file.includes('@theme/')) return 'theme';
  if (file.includes('clusters') || file.includes('dashboard')) return 'clusters';
  if (file.includes('backends') || file.includes('frontends')) return 'nodes';
  if (file.includes('queries') || file.includes('sessions')) return file.includes('sessions') ? 'sessions' : 'queries';
  if (file.includes('variables')) return 'variables';
  if (file.includes('materialized-views')) return 'mv';
  if (file.includes('system/')) return 'system';
  if (file.includes('permission-management')) return 'permission';
  if (file.includes('resource-groups')) return 'resource-groups';
  if (file.includes('agent')) return 'agent';
  return 'common';
}

const CJK = /[\u4e00-\u9fff]/;

function extractFromHtml(file: string, content: string): ExtractedString[] {
  const out: ExtractedString[] = [];
  const lines = content.split('\n');
  lines.forEach((line, i) => {
    // 标签间文本 >中文<
    for (const m of line.matchAll(/>([^<>]*[\u4e00-\u9fff][^<>]*)</g)) {
      const text = m[1].trim().replace(/\{\{[^}]*\}\}/g, '').trim();
      if (text) out.push({ file, line: i + 1, text, kind: 'html-text', ns: nsFor(file) });
    }
    // 属性值 placeholder/title/tooltip="中文"
    for (const m of line.matchAll(/(?:placeholder|title|nbTooltip|aria-label|label)="([^"]*[\u4e00-\u9fff][^"]*)"/g)) {
      out.push({ file, line: i + 1, text: m[1], kind: 'attr', ns: nsFor(file) });
    }
  });
  return out;
}

function extractFromTs(file: string, content: string): ExtractedString[] {
  const out: ExtractedString[] = [];
  const lines = content.split('\n');
  lines.forEach((line, i) => {
    if (line.trim().startsWith('*') || line.trim().startsWith('//')) return; // 注释跳过
    // 单引号/反引号字符串里的中文
    for (const m of line.matchAll(/'([^']*[一-龥][^']*)'/g)) {
      out.push({ file, line: i + 1, text: m[1], kind: 'ts-string', ns: nsFor(file) });
    }
    for (const m of line.matchAll(/`([^`]*[一-龥][^`]*)`/g)) {
      const t = m[1].replace(/\$\{[^}]*\}/g, 'X');
      if (t.trim()) out.push({ file, line: i + 1, text: t, kind: 'ts-template', ns: nsFor(file) });
    }
  });
  return out;
}

// CLI 入口：node --experimental-strip-types scripts/i18n-extract.ts
if (typeof require !== 'undefined' && require.main === module) {
  void (async () => {
    const { glob } = await import('glob');
    const files = await glob('src/app/**/*.{html,ts}', { ignore: ['**/*.spec.ts', '**/i18n/**'] });
    const all: ExtractedString[] = [];
    for (const f of files) {
      const content = require('fs').readFileSync(f, 'utf8');
      all.push(...(f.endsWith('.html') ? extractFromHtml(f, content) : extractFromTs(f, content)));
    }
    // 按命名空间聚合 unique 文案
    const byNs: Record<string, Set<string>> = {};
    for (const e of all) {
      (byNs[e.ns] ??= new Set()).add(e.text);
    }
    let total = 0;
    for (const [ns, set] of Object.entries(byNs)) {
      const dict: Record<string, string> = {};
      for (const t of set) dict[t] = t; // 占位：待翻译
      total += set.size;
      require('fs').writeFileSync(`src/assets/i18n/work/${ns}.pending.json`, JSON.stringify(dict, null, 2) + '\n');
      console.log(`${ns}: ${set.size}`);
    }
    console.log('TOTAL unique strings:', total);
  })();
}
