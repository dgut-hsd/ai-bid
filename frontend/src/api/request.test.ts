import { describe, expect, it } from 'vitest';
import { extractErrorCode } from './request';

describe('request error handling', () => {
  it('reads tenant error codes from the backend error data envelope', () => {
    expect(
      extractErrorCode({ data: { error_code: 'TENANT_REQUIRED' } })
    ).toBe('TENANT_REQUIRED');
  });
});
