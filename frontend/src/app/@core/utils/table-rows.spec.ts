import { LocalDataSource } from 'angular2-smart-table';
import { assignTableRows } from './table-rows';

describe('assignTableRows', () => {
  it('loads rows and resolves without waiting for onChanged', async () => {
    const source = new LocalDataSource();
    source.onChanged = () => {
      throw new Error('onChanged must not gate assignTableRows');
    };

    await assignTableRows(source, [{ id: 1 }]);

    await expectAsync(source.getAll()).toBeResolvedTo([{ id: 1 }]);
  });

  it('loads an empty list when rows is not an array', async () => {
    const source = new LocalDataSource();

    await assignTableRows(source, null as unknown as unknown[]);

    await expectAsync(source.getAll()).toBeResolvedTo([]);
  });
});
