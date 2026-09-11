export interface ChartChrome {
  primary: string;
  success: string;
  info: string;
  warning: string;
  danger: string;
  cardBg: string;
  textBasic: string;
  textHint: string;
  border: string;
}

export interface BarItem {
  name: string;
  value: number;
}

function tooltipBox(chrome: ChartChrome) {
  return {
    backgroundColor: chrome.cardBg,
    borderColor: chrome.border,
    borderWidth: 1,
    textStyle: {
      color: chrome.textBasic,
      fontSize: 11,
    },
  };
}

export function donutOption(
  chrome: ChartChrome,
  items: Array<BarItem & { color: string }>,
  center: { value: string; name: string },
): Record<string, unknown> {
  const slices = items.filter(item => item.value > 0);
  return {
    tooltip: {
      trigger: 'item',
      ...tooltipBox(chrome),
      formatter: (param: { name: string; value: number; percent: number; marker: string }) =>
        `${param.marker} ${param.name}<br/>${param.value}（${param.percent}%）`,
    },
    legend: {
      bottom: 4,
      left: 'center',
      icon: 'circle',
      itemWidth: 8,
      itemHeight: 8,
      textStyle: {
        color: chrome.textHint,
        fontSize: 11,
      },
    },
    series: [
      {
        type: 'pie',
        radius: ['54%', '74%'],
        center: ['50%', '44%'],
        avoidLabelOverlap: false,
        label: {
          show: true,
          position: 'center',
          formatter: `{v|${center.value}}\n{n|${center.name}}`,
          rich: {
            v: {
              color: chrome.textBasic,
              fontSize: 22,
              fontWeight: 600,
              lineHeight: 28,
            },
            n: {
              color: chrome.textHint,
              fontSize: 11,
              lineHeight: 18,
            },
          },
        },
        labelLine: { show: false },
        data: slices.map(item => ({
          name: item.name,
          value: item.value,
          itemStyle: { color: item.color },
        })),
      },
    ],
  };
}

export function horizontalBarOption(
  chrome: ChartChrome,
  items: BarItem[],
  color: string,
  formatValue?: (value: number) => string,
  max?: number,
): Record<string, unknown> {
  const categories = items.map(item => item.name).reverse();
  const values = items.map(item => item.value).reverse();
  return {
    grid: {
      left: 8,
      right: 48,
      top: 8,
      bottom: 8,
      containLabel: true,
    },
    tooltip: {
      trigger: 'axis',
      axisPointer: { type: 'shadow' },
      ...tooltipBox(chrome),
      formatter: (params: Array<{ name: string; value: number; marker: string }>) => {
        const point = params[0];
        const formatted = formatValue ? formatValue(point.value) : String(point.value);
        return `${point.marker} ${point.name}<br/>${formatted}`;
      },
    },
    xAxis: {
      type: 'value',
      max,
      axisLabel: {
        color: chrome.textHint,
        fontSize: 11,
        formatter: formatValue,
      },
      axisLine: { show: false },
      splitLine: {
        lineStyle: { color: chrome.border },
      },
    },
    yAxis: {
      type: 'category',
      data: categories,
      axisLabel: {
        color: chrome.textBasic,
        fontSize: 11,
        width: 112,
        overflow: 'truncate',
      },
      axisTick: { show: false },
      axisLine: { show: false },
    },
    series: [
      {
        type: 'bar',
        data: values,
        barMaxWidth: 16,
        itemStyle: {
          color,
          borderRadius: [0, 4, 4, 0],
        },
        label: {
          show: true,
          position: 'right',
          color: chrome.textHint,
          fontSize: 10,
          formatter: (param: { value: number }) => (formatValue ? formatValue(param.value) : param.value),
        },
      },
    ],
  };
}

