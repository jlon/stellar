export function tableRowData<T = any>(cell: any): T {
  if (cell && typeof cell.getRow === 'function') {
    return cell.getRow().getData() as T;
  }
  return cell as T;
}

export function withTableRow<V, R, T>(fn: (value: V, row: R) => T) {
  return (value: V, cell: any) => fn(value, tableRowData<R>(cell));
}
