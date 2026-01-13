export type FieldType = 'numeric' | 'text';
export type AggregationType = 'COUNT' | 'SUM' | 'AVG' | 'MAX' | 'MIN';
export type ChartType = 'bar' | 'line' | 'pie';

export interface ChartField {
  name: string;
  type: FieldType;
  columnIndex: number;
}

export interface ChartConfig {
  id: string;
  title: string;
  fieldName: string;
  fieldType: FieldType;
  chartType: ChartType;
  aggregation: AggregationType;
  columnIndex: number;
}

export interface ChartDataPoint {
  label: string;
  value: number;
}

export function inferFieldType(values: string[]): FieldType {
  const sampleSize = Math.min(values.length, 100);
  const sample = values.slice(0, sampleSize);
  const nonEmptyValues = sample.filter(v => v !== null && v !== '' && v !== 'NULL');
  
  if (nonEmptyValues.length === 0) {
    return 'text';
  }
  
  const allNumeric = nonEmptyValues.every(v => !isNaN(parseFloat(v)) && isFinite(Number(v)));
  return allNumeric ? 'numeric' : 'text';
}

export function generateChartData(
  rows: string[][],
  columnIndex: number,
  fieldType: FieldType,
  aggregation: AggregationType
): ChartDataPoint[] {
  const groups = new Map<string, number[]>();

  for (const row of rows) {
    const value = row[columnIndex] || 'null';
    if (!groups.has(value)) {
      groups.set(value, []);
    }
    if (fieldType === 'numeric') {
      const num = parseFloat(row[columnIndex]);
      if (!isNaN(num)) {
        groups.get(value)!.push(num);
      }
    } else {
      groups.get(value)!.push(1);
    }
  }

  const result: ChartDataPoint[] = [];
  for (const [label, values] of groups) {
    let aggregatedValue: number;
    switch (aggregation) {
      case 'COUNT':
        aggregatedValue = values.length;
        break;
      case 'SUM':
        aggregatedValue = values.reduce((a, b) => a + b, 0);
        break;
      case 'AVG':
        aggregatedValue = values.length > 0 ? values.reduce((a, b) => a + b, 0) / values.length : 0;
        break;
      case 'MAX':
        aggregatedValue = values.length > 0 ? Math.max(...values) : 0;
        break;
      case 'MIN':
        aggregatedValue = values.length > 0 ? Math.min(...values) : 0;
        break;
      default:
        aggregatedValue = values.length;
    }
    result.push({ label, value: aggregatedValue });
  }

  result.sort((a, b) => b.value - a.value);
  return result.slice(0, 20);
}
