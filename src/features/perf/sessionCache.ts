/** Memoises a loader's promise for the session; a rejected load is forgotten so the next call tries again. */
export function sessionCache<T>(load: () => Promise<T>): () => Promise<T> {
  let cached: Promise<T> | null = null;
  return () => {
    cached ??= load().catch((error: unknown) => {
      cached = null;
      throw error;
    });
    return cached;
  };
}
