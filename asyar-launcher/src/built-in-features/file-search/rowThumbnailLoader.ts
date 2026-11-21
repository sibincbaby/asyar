export type RowThumbnail = {
  url: string | null;
};

type LoaderDependencies = {
  getThumbnail: (path: string, maxDim: number) => Promise<string | null>;
  thumbnailDimension?: number;
};

export function createRowThumbnailLoader({
  getThumbnail,
  thumbnailDimension = 56,
}: LoaderDependencies) {
  const inFlight = new Map<string, Promise<RowThumbnail>>();

  function load(fileId: string, path: string, isImage: boolean): Promise<RowThumbnail> {
    const pending = inFlight.get(fileId);
    if (pending) return pending;

    const request = (async (): Promise<RowThumbnail> => {
      if (!isImage) return { url: null };
      return { url: await getThumbnail(path, thumbnailDimension).catch(() => null) };
    })().finally(() => inFlight.delete(fileId));

    inFlight.set(fileId, request);
    return request;
  }

  return {
    load,
    isInFlight: (fileId: string) => inFlight.has(fileId),
  };
}
