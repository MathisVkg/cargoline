/** Static placeholder rows while a page's code loads (Task 18). No animation: nothing runs. */
export function PageSkeleton() {
  return (
    <div className="mx-auto w-full max-w-5xl px-4 py-6 sm:px-6 sm:py-8" aria-busy="true" aria-label="Loading page">
      <div className="mb-6 h-6 w-48 rounded-sm bg-raised" />
      {[0, 1, 2].map((i) => (
        <div key={i} className="mb-3 h-16 rounded-sm border border-line bg-surface" />
      ))}
    </div>
  );
}
