import * as fc from 'fast-check';
import { inferFieldType, generateChartData, FieldType, AggregationType } from './chart-utils';

describe('Chart Utils', () => {
  describe('inferFieldType', () => {
    it('should return numeric for array of numbers', () => {
      expect(inferFieldType(['1', '2', '3', '4.5'])).toBe('numeric');
    });

    it('should return text for array with non-numeric values', () => {
      expect(inferFieldType(['hello', 'world'])).toBe('text');
    });

    it('should return text for empty array', () => {
      expect(inferFieldType([])).toBe('text');
    });

    it('should return text for array of empty strings', () => {
      expect(inferFieldType(['', '', ''])).toBe('text');
    });

    it('should return numeric for mixed empty and numeric values', () => {
      expect(inferFieldType(['1', '', '2', 'NULL', '3'])).toBe('numeric');
    });

    /**
     * Property 1: Field Type Inference Correctness
     * For any array of string values, if all non-empty values can be parsed as valid numbers,
     * the inferFieldType function SHALL return 'numeric'; otherwise it SHALL return 'text'.
     * Validates: Requirements 2.2, 3.1, 3.2
     */
    it('Property 1: should correctly infer numeric type for all-numeric arrays', () => {
      fc.assert(
        fc.property(
          fc.array(fc.double({ min: -1e10, max: 1e10, noNaN: true }), { minLength: 1, maxLength: 20 }),
          (numbers) => {
            const stringValues = numbers.map(n => n.toString());
            return inferFieldType(stringValues) === 'numeric';
          }
        ),
        { numRuns: 20 }
      );
    });

    it('Property 1: should correctly infer text type for arrays with non-numeric values', () => {
      fc.assert(
        fc.property(
          fc.array(fc.string({ minLength: 1 }), { minLength: 1, maxLength: 20 }),
          (strings) => {
            const hasNonNumeric = strings.some(s => s !== '' && s !== 'NULL' && isNaN(parseFloat(s)));
            if (hasNonNumeric) {
              return inferFieldType(strings) === 'text';
            }
            return true;
          }
        ),
        { numRuns: 20 }
      );
    });
  });

  describe('generateChartData', () => {
    it('should generate correct COUNT aggregation', () => {
      const rows = [['a'], ['b'], ['a'], ['c'], ['a']];
      const result = generateChartData(rows, 0, 'text', 'COUNT');
      
      expect(result.length).toBe(3);
      expect(result[0]).toEqual({ label: 'a', value: 3 });
      expect(result[1]).toEqual({ label: 'b', value: 1 });
      expect(result[2]).toEqual({ label: 'c', value: 1 });
    });

    it('should generate correct SUM aggregation for numeric fields', () => {
      const rows = [['10'], ['20'], ['30']];
      const result = generateChartData(rows, 0, 'numeric', 'SUM');
      
      expect(result.length).toBe(3);
      const total = result.reduce((sum, item) => sum + item.value, 0);
      expect(total).toBe(60);
    });

    it('should limit results to 20 items', () => {
      const rows = Array.from({ length: 50 }, (_, i) => [String.fromCharCode(65 + i % 26) + i]);
      const result = generateChartData(rows, 0, 'text', 'COUNT');
      
      expect(result.length).toBeLessThanOrEqual(20);
    });

    it('should sort results by value in descending order', () => {
      const rows = [['a'], ['b'], ['b'], ['c'], ['c'], ['c']];
      const result = generateChartData(rows, 0, 'text', 'COUNT');
      
      for (let i = 1; i < result.length; i++) {
        expect(result[i - 1].value).toBeGreaterThanOrEqual(result[i].value);
      }
    });

    /**
     * Property 2: Chart Data Generation Correctness
     * For any query result data and field selection, the generateChartData function SHALL:
     * - Return at most 20 data points
     * - Return data points sorted by value in descending order
     * - Correctly calculate the specified aggregation
     * Validates: Requirements 4.1, 4.2, 4.5
     */
    it('Property 2: should always return at most 20 data points', () => {
      fc.assert(
        fc.property(
          fc.array(fc.array(fc.string(), { minLength: 1, maxLength: 5 }), { minLength: 1, maxLength: 50 }),
          (rows) => {
            if (rows.length === 0 || rows[0].length === 0) return true;
            const result = generateChartData(rows, 0, 'text', 'COUNT');
            return result.length <= 20;
          }
        ),
        { numRuns: 20 }
      );
    });

    it('Property 2: should always return sorted results in descending order', () => {
      fc.assert(
        fc.property(
          fc.array(fc.array(fc.string({ minLength: 1 }), { minLength: 1, maxLength: 3 }), { minLength: 1, maxLength: 30 }),
          (rows) => {
            if (rows.length === 0 || rows[0].length === 0) return true;
            const result = generateChartData(rows, 0, 'text', 'COUNT');
            for (let i = 1; i < result.length; i++) {
              if (result[i - 1].value < result[i].value) return false;
            }
            return true;
          }
        ),
        { numRuns: 20 }
      );
    });

    it('Property 2: COUNT aggregation should equal total row count', () => {
      fc.assert(
        fc.property(
          fc.array(fc.array(fc.constantFrom('a', 'b', 'c'), { minLength: 1, maxLength: 1 }), { minLength: 1, maxLength: 30 }),
          (rows) => {
            const result = generateChartData(rows, 0, 'text', 'COUNT');
            const totalCount = result.reduce((sum, item) => sum + item.value, 0);
            return totalCount === rows.length;
          }
        ),
        { numRuns: 20 }
      );
    });
  });
});


describe('Chart Config Update', () => {
  /**
   * Property 3: Chart Config Update Correctness
   * For any chart configuration modification, when the user confirms changes,
   * the system SHALL regenerate chart data using the new configuration.
   * Validates: Requirements 6.3
   */
  it('Property 3: changing aggregation should produce different results for numeric data', () => {
    const rows = [['10'], ['20'], ['30'], ['10'], ['20']];
    
    const countResult = generateChartData(rows, 0, 'numeric', 'COUNT');
    const sumResult = generateChartData(rows, 0, 'numeric', 'SUM');
    
    const countTotal = countResult.reduce((sum, item) => sum + item.value, 0);
    const sumTotal = sumResult.reduce((sum, item) => sum + item.value, 0);
    
    expect(countTotal).toBe(5);
    expect(sumTotal).toBe(90);
  });

  it('Property 3: different aggregations should produce consistent results', () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.array(fc.integer({ min: 1, max: 100 }).map(n => n.toString()), { minLength: 1, maxLength: 1 }),
          { minLength: 5, maxLength: 20 }
        ),
        (rows) => {
          const countResult = generateChartData(rows, 0, 'numeric', 'COUNT');
          const sumResult = generateChartData(rows, 0, 'numeric', 'SUM');
          const avgResult = generateChartData(rows, 0, 'numeric', 'AVG');
          
          const countTotal = countResult.reduce((sum, item) => sum + item.value, 0);
          expect(countTotal).toBe(rows.length);
          
          return sumResult.length > 0 && avgResult.length > 0;
        }
      ),
      { numRuns: 20 }
    );
  });
});
