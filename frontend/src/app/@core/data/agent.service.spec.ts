import { parseSseEvent } from './agent.service';

describe('parseSseEvent', () => {
  it('restores Markdown line breaks split into SSE data lines', () => {
    const event = parseSseEvent(
      [
        'event: answer',
        'data: ## 依据',
        'data: ',
        'data: | 指标 | 当前值 |',
        'data: | --- | --- |',
        'data: | 磁盘 | 98.6% |',
      ].join('\n'),
    );

    expect(event).toEqual({
      type: 'answer',
      final_answer: '## 依据\n\n| 指标 | 当前值 |\n| --- | --- |\n| 磁盘 | 98.6% |',
    });
  });

  it('preserves leading whitespace in a streamed code token', () => {
    const event = parseSseEvent('event: delta\ndata:   SELECT 1');

    expect(event).toEqual({ type: 'delta', text: '  SELECT 1' });
  });
});
