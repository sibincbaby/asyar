import { describe, expect, it, vi } from 'vitest';
import { createRowThumbnailLoader } from './rowThumbnailLoader';

describe('row thumbnail loader', () => {
  it('deduplicates concurrent requests for the same file', async () => {
    let resolveThumb: (url: string | null) => void = () => {};
    const getThumbnail = vi.fn(
      () =>
        new Promise<string | null>((resolve) => {
          resolveThumb = resolve;
        }),
    );
    const loader = createRowThumbnailLoader({ getThumbnail });

    const first = loader.load('file-1', '/one/photo.png', true);
    const second = loader.load('file-1', '/one/photo.png', true);
    expect(loader.isInFlight('file-1')).toBe(true);
    resolveThumb('asyar-thumb://localhost/photo.png');

    await expect(first).resolves.toEqual({ url: 'asyar-thumb://localhost/photo.png' });
    await expect(second).resolves.toEqual({ url: 'asyar-thumb://localhost/photo.png' });
    expect(getThumbnail).toHaveBeenCalledOnce();
    expect(loader.isInFlight('file-1')).toBe(false);
  });

  it('requests no thumbnail for non-image files', async () => {
    const getThumbnail = vi.fn();
    const loader = createRowThumbnailLoader({ getThumbnail });

    await expect(loader.load('file-1', '/one/report.pdf', false)).resolves.toEqual({ url: null });
    expect(getThumbnail).not.toHaveBeenCalled();
  });

  it('falls back to no image when thumbnail generation fails', async () => {
    const loader = createRowThumbnailLoader({
      getThumbnail: vi.fn().mockRejectedValue(new Error('boom')),
    });

    await expect(loader.load('image-1', '/one/photo.png', true)).resolves.toEqual({ url: null });
  });
});
