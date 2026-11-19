import { CalendarDays, Trash2 } from "lucide-react";
import { DisabledButton, EmptyState, PageContainer, PageHeader, Panel, PanelHeader, Tag } from "../components/ui";

const STATS = [
  { label: "Completed SCU", unit: "SCU" },
  { label: "Completed contracts", unit: "" },
  { label: "Estimated earned", unit: "aUEC" },
] as const;

const COLUMNS = ["Date", "Title", "SCU", "aUEC", "Status"] as const;

export function HistoryPage() {
  return (
    <PageContainer>
      <PageHeader
        index="03"
        title="History"
        description="Contracts grouped into numbered runs by local acceptance date."
        actions={
          <DisabledButton icon={Trash2} note="History is not available yet">
            Clear History
          </DisabledButton>
        }
      />

      <section aria-label="Lifetime statistics" className="mb-6 grid grid-cols-1 gap-px overflow-hidden rounded-sm border border-line bg-line sm:grid-cols-3">
        {STATS.map(({ label, unit }) => (
          <div key={label} className="min-w-0 bg-surface px-4 py-4">
            <p className="label-tech truncate">{label}</p>
            <p className="mt-1 font-mono text-2xl text-dim">
              —{unit && <span className="ml-1.5 text-sm">{unit}</span>}
            </p>
          </div>
        ))}
      </section>

      <Panel>
        <PanelHeader title="Runs" meta={<Tag>Not available yet</Tag>} />
        <div className="overflow-x-auto">
          <table className="w-full min-w-[560px] border-collapse text-left text-sm">
            <thead>
              <tr className="border-b border-line">
                {COLUMNS.map((col) => (
                  <th
                    key={col}
                    scope="col"
                    className={["label-tech px-4 py-2 font-normal", col === "SCU" || col === "aUEC" ? "text-right" : ""].join(" ")}
                  >
                    {col}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              <tr>
                <td colSpan={COLUMNS.length}>
                  <EmptyState icon={CalendarDays} title="No runs recorded yet">
                    A run is created automatically on the first contract of each local date. Completed and abandoned
                    contracts will be listed here.
                  </EmptyState>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </Panel>
    </PageContainer>
  );
}
