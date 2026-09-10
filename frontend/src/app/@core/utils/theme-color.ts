export function themeColor(cssVar: string, fallback: string): string {
  const themeRoot = (document.querySelector('nb-layout') || document.body) as HTMLElement;
  return getComputedStyle(themeRoot).getPropertyValue(cssVar).trim() || fallback;
}

export function themeColorAlpha(cssVar: string, alpha: number, fallbackHex: string): string {
  return colorWithAlpha(themeColor(cssVar, fallbackHex), alpha);
}

export function colorWithAlpha(color: string, alpha: number): string {
  const hex = color.replace('#', '').trim();
  if (/^[0-9a-fA-F]{6}$/.test(hex)) {
    const r = parseInt(hex.slice(0, 2), 16);
    const g = parseInt(hex.slice(2, 4), 16);
    const b = parseInt(hex.slice(4, 6), 16);
    return `rgba(${r}, ${g}, ${b}, ${alpha})`;
  }
  const rgb = color.match(/rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/);
  if (rgb) {
    return `rgba(${rgb[1]}, ${rgb[2]}, ${rgb[3]}, ${alpha})`;
  }
  return color;
}

export function themeChartChrome() {
  return {
    primary: themeColor('--color-primary-default', '#3366ff'),
    success: themeColor('--color-success-default', '#00d68f'),
    info: themeColor('--color-info-default', '#0095ff'),
    warning: themeColor('--color-warning-default', '#ffaa00'),
    danger: themeColor('--color-danger-default', '#ff3d71'),
    cardBg: themeColor('--background-basic-color-1', '#ffffff'),
    textBasic: themeColor('--text-basic-color', '#222b45'),
    textHint: themeColor('--text-hint-color', '#8f9bb3'),
    border: themeColor('--border-basic-color-3', '#e4e9f2'),
  };
}
