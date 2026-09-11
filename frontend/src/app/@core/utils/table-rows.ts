import { LocalDataSource } from 'angular2-smart-table';

export function assignTableRows<T>(source: LocalDataSource, rows: T[]): Promise<void> {
  const list = Array.isArray(rows) ? rows : [];
  try {
    void source.load(list);
  } catch {
    try {
      void source.load([]);
    } catch {
    }
  }
  return Promise.resolve();
}
